use ofborg::checkout;
use ofborg::config;
use ofborg::easyamqp::{self, ChannelExt, ConsumerExt};
use ofborg::stats;
use ofborg::tasks;
use ofborg::worker;

use std::env;
use std::path::Path;
use std::process;

use amqp::Basic;
use tracing::{error, info};

// FIXME: remove with rust/cargo update
#[allow(clippy::cognitive_complexity)]
fn main() {
    let memory_info = sys_info::mem_info().expect("Unable to get memory information from OS");

    if memory_info.avail < 8 * 1024 * 1024 {
        // seems this stuff is in kilobytes?
        error!(
            "Less than 8Gb of memory available (got {:.2}Gb). Aborting.",
            (memory_info.avail as f32) / 1024.0 / 1024.0
        );
        process::exit(1);
    };

    let cfg = config::load(env::args().nth(1).unwrap().as_ref());

    ofborg::setup_log();

    info!("Hello, world!");

    let mut session = easyamqp::session_from_config(&cfg.rabbitmq).unwrap();
    info!("Connected to rabbitmq");

    let mut channel = session.open_channel(1).unwrap();

    let cloner = checkout::cached_cloner(Path::new(&cfg.checkout.root));
    let nix = cfg.nix();

    let events = stats::RabbitMQ::new(
        &format!("{}-{}", cfg.runner.identity.clone(), cfg.nix.system.clone()),
        session.open_channel(3).unwrap(),
    );

    let mrw = tasks::evaluate::EvaluationWorker::new(
        cloner,
        &nix,
        cfg.github(),
        cfg.github_app_vendingmachine(),
        cfg.acl(),
        cfg.runner.identity.clone(),
        events,
        cfg.tag_paths.clone().unwrap(),
    );

    channel
        .declare_queue(easyamqp::QueueConfig {
            queue: "mass-rebuild-check-jobs".to_owned(),
            passive: false,
            durable: true,
            exclusive: false,
            auto_delete: false,
            no_wait: false,
        })
        .unwrap();

    channel.basic_prefetch(1).unwrap();
    let mut channel = channel
        .consume(
            worker::new(mrw),
            easyamqp::ConsumeConfig {
                queue: "mass-rebuild-check-jobs".to_owned(),
                consumer_tag: format!("{}-mass-rebuild-checker", cfg.whoami()),
                no_local: false,
                no_ack: false,
                no_wait: false,
                exclusive: false,
            },
        )
        .unwrap();

    channel.start_consuming();

    info!("Finished consuming?");

    channel.close(200, "Bye").unwrap();
    info!("Closed the channel");
    session.close(200, "Good Bye");
    info!("Closed the session... EOF");
}
