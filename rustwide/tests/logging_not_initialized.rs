use log::{info, LevelFilter};
use rustwide::logging::{self, LogStorage};

#[test]
#[should_panic = "called capture without initializing rustwide::logging"]
fn test_not_initialized() {
    let storage = LogStorage::new(LevelFilter::Info);
    logging::capture(&storage, || {
        info!("Hello world");
    });
}
