//! Background autoroute job contract used by SSE.

use anvil_studio::api::{job_snapshot, spawn_route, StudioState};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[test]
fn background_route_finishes_and_publishes_a_report() {
    let state = Arc::new(Mutex::new(StudioState::new()));
    spawn_route(state.clone()).unwrap();
    let start = Instant::now();
    loop {
        let (seq, job, done) = job_snapshot(&state);
        assert!(seq >= 1);
        assert!(job.get("phase").and_then(|v| v.as_str()).is_some());
        if done {
            let st = state.lock().unwrap();
            assert!(!st.job.running);
            assert!(st.job.finished);
            let report = st.job.report.as_ref().expect("report");
            assert!(report.clean, "{:?}", report.drc.violations);
            assert!(st.board.is_some());
            assert!(st.log.iter().any(|l| l.contains("autoroute finished")));
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "route did not finish");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn starting_twice_conflicts() {
    let state = Arc::new(Mutex::new(StudioState::new()));
    {
        let mut st = state.lock().unwrap();
        st.job.running = true;
    }
    assert!(spawn_route(state.clone()).is_err());
}

#[test]
fn rerouting_after_completion_is_possible() {
    let state = Arc::new(Mutex::new(StudioState::new()));
    spawn_route(state.clone()).unwrap();
    let start = Instant::now();
    while !job_snapshot(&state).2 {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(20));
    }
    // A second run clears and replaces the routes deterministically.
    spawn_route(state.clone()).unwrap();
    let start = Instant::now();
    while !job_snapshot(&state).2 {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(20));
    }
    let st = state.lock().unwrap();
    assert!(st.job.report.as_ref().unwrap().clean);
    assert!(st.board.as_ref().unwrap().traces.len() >= 1);
}
