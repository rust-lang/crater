use log::{info, LevelFilter};
use rustwide::logging::{self, LogStorage};

#[test]
#[should_panic = "called capture without initializing rustwide::logging"]
fn test_not_initialized() {
    let storage = LogStorage::new(LevelFilter::Info, 1024, 100);
    logging::capture(&storage, || {
        info!("Hello world");
    });
}
