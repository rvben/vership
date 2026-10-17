use super::{CheckResult, MAX_VERIFY_WORKERS, check_targets, targets::Target};
use std::sync::{Condvar, Mutex, mpsc};
use std::time::Duration;

#[test]
fn checks_overlap_with_a_bound_and_return_in_plan_order() {
    let targets: Vec<_> = (0..12)
        .map(|i| Target::Crates {
            name: i.to_string(),
        })
        .collect();
    let (tx, rx) = mpsc::channel();
    let release = (Mutex::new(false), Condvar::new());
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            check_targets(&targets, |target| {
                let Target::Crates { name } = target else {
                    unreachable!()
                };
                tx.send(name.clone()).unwrap();
                let (lock, ready) = &release;
                let _guard = ready
                    .wait_while(lock.lock().unwrap(), |released| !*released)
                    .unwrap();
                CheckResult::Found(name.clone())
            })
        });
        // Keep the first wave blocked. Serial execution cannot reach four;
        // an unbounded implementation starts a fifth before we release them.
        let started: Vec<_> = (0..MAX_VERIFY_WORKERS)
            .map(|_| rx.recv_timeout(Duration::from_secs(5)))
            .collect();
        let extra = rx.recv_timeout(Duration::from_millis(100));
        *release.0.lock().unwrap() = true;
        release.1.notify_all();
        let reports = worker.join().unwrap();
        assert!(started.iter().all(Result::is_ok), "first wave: {started:?}");
        assert!(
            matches!(extra, Err(mpsc::RecvTimeoutError::Timeout)),
            "extra worker: {extra:?}"
        );
        assert_eq!(reports.len(), targets.len());
        for (i, report) in reports.iter().enumerate() {
            assert!(report.ok);
            assert_eq!(report.found.as_deref(), Some(i.to_string().as_str()));
        }
        assert_eq!(rx.try_iter().count(), targets.len() - MAX_VERIFY_WORKERS);
    });
}

#[test]
fn all_target_failures_are_collected_in_order() {
    let targets = [
        Target::Tag,
        Target::Release,
        Target::Npm {
            name: "fixture".into(),
        },
    ];
    let reports = check_targets(&targets, |target| match target {
        Target::Tag => CheckResult::NotFound,
        Target::Release => CheckResult::Error("offline".into()),
        _ => CheckResult::FoundOld("0.9.0".into()),
    });
    assert_eq!(
        reports.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["tag", "release", "npm"]
    );
    assert!(reports.iter().all(|r| !r.ok));
    assert_eq!(reports[0].detail.as_deref(), Some("not found"));
    assert_eq!(reports[1].detail.as_deref(), Some("check error: offline"));
    assert_eq!(reports[2].found.as_deref(), Some("0.9.0"));
}

#[test]
fn empty_and_single_target_need_no_worker() {
    assert!(check_targets(&[], |_| panic!("empty plan ran a check")).is_empty());
    let caller = std::thread::current().id();
    let reports = check_targets(&[Target::Tag], |_| {
        assert_eq!(std::thread::current().id(), caller);
        CheckResult::Found("v1.0.0".into())
    });
    assert!(reports[0].ok);
}

#[test]
fn noncontiguous_worker_results_return_in_plan_order() {
    let targets: Vec<_> = (0..MAX_VERIFY_WORKERS * 2)
        .map(|i| Target::Crates {
            name: i.to_string(),
        })
        .collect();
    let state = (Mutex::new((0, false)), Condvar::new());
    let reports = check_targets(&targets, |target| {
        let Target::Crates { name } = target else {
            unreachable!()
        };
        let i: usize = name.parse().unwrap();
        let (lock, ready) = &state;
        let mut progress = lock.lock().unwrap();
        if i < MAX_VERIFY_WORKERS {
            progress.0 += 1;
            ready.notify_all();
            let (_guard, timeout) = ready
                .wait_timeout_while(progress, Duration::from_secs(5), |s| {
                    if i == 0 {
                        s.0 < MAX_VERIFY_WORKERS
                    } else {
                        !s.1
                    }
                })
                .unwrap();
            assert!(!timeout.timed_out(), "first wave did not make progress");
        } else if i == MAX_VERIFY_WORKERS {
            progress.1 = true;
            ready.notify_all();
        }
        CheckResult::Found(name.clone())
    });
    // The worker that processed 0 must also process MAX_VERIFY_WORKERS while
    // the intervening workers are held. Its results cannot form a contiguous
    // segment, so concatenating worker vectors without sorting always fails.
    assert_eq!(
        reports
            .iter()
            .map(|r| r.found.clone().unwrap())
            .collect::<Vec<_>>(),
        (0..targets.len())
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
    );
}
