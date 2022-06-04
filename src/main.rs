use std::error::Error;
use std::fmt::{Debug, Display};
use std::sync::atomic;
use std::sync::Arc;
use std::thread;
use std::time;

use log::{debug, error, info};
use notify_rust::Notification;

const NOTIFICATION_TIMEOUT: i32 = 3000;
const LOOP_WAIT_TIME: time::Duration = time::Duration::from_secs(1);
const CRITICAL_CHARGE: f32 = 0.15;

#[derive(Debug)]
struct BatteryStatus {
    state: battery::State,
    time_to_full: Option<battery::units::Time>,
    charge: f32,
}

#[derive(Debug)]
enum BatteryError {
    FailedToGetState,
    LibError(battery::Error),
}

impl Display for BatteryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BatteryError::FailedToGetState => write!(f, "failed to get battery state"),
            BatteryError::LibError(e) => write!(f, "{:?}", e),
        }
    }
}

impl Error for BatteryError {}

impl From<battery::Error> for BatteryError {
    fn from(e: battery::Error) -> Self {
        BatteryError::LibError(e)
    }
}

fn get_battery_state() -> Result<BatteryStatus, BatteryError> {
    let manager = battery::Manager::new()?;
    if let Some(bat) = manager.batteries()?.take(1).next().transpose()? {
        Ok(BatteryStatus {
            state: bat.state(),
            time_to_full: bat.time_to_full(),
            charge: bat.state_of_charge().value,
        })
    } else {
        Err(BatteryError::FailedToGetState)
    }
}

fn get_battery_state_changed_notif(
    state: battery::State,
    time_to_charge: Option<battery::units::Time>,
) -> Notification {
    let mut n = Notification::new()
        .summary(&format!("Battery state - {:?}", state))
        .timeout(NOTIFICATION_TIMEOUT)
        .finalize();
    if let Some(ttc) = time_to_charge {
        let duration =
            time::Duration::from_nanos(ttc.get::<battery::units::time::nanosecond>() as u64);
        return n
            .body(&format!("{}m", (duration.as_secs() / 60) % 60))
            .finalize();
    }

    n
}

fn get_battery_low_notif(val: f32) -> Notification {
    Notification::new()
        .summary("Battery charge is critically low")
        .body(&format!("charge - {}%", val * 100_f32))
        .timeout(NOTIFICATION_TIMEOUT)
        .finalize()
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let t = time::Duration::from_secs(1);
    println!("{:?}", t);
    let mut state = get_battery_state()?;
    debug!("got initial battery state");
    let running = Arc::new(atomic::AtomicBool::new(true));
    let running_clone = running.clone();
    ctrlc::set_handler(move || running_clone.store(false, atomic::Ordering::Relaxed))
        .expect("failed to set ctrl-c trap");
    info!("start fetching state every {:?}", LOOP_WAIT_TIME);
    let mut is_low_notified = false;
    loop {
        thread::sleep(LOOP_WAIT_TIME);
        if !running.load(atomic::Ordering::Relaxed) {
            info!("ctrl-c catched. exiting...");
            break;
        }

        match get_battery_state() {
            Ok(new_state) => {
                if new_state.state != state.state {
                    get_battery_state_changed_notif(new_state.state, new_state.time_to_full)
                        .show()?;
                    debug!("new battery state {:?}", new_state.state);
                }
                if new_state.charge <= CRITICAL_CHARGE {
                    if !is_low_notified {
                        get_battery_low_notif(new_state.charge).show()?;
                        is_low_notified = true;
                        debug!("charge is lower then 15% - {}", new_state.charge);
                    }
                } else {
                    is_low_notified = false;
                }

                state = new_state;
            }
            Err(e) => error!("{:?}", e),
        };
    }
    Ok(())
}
