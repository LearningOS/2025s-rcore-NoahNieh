use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
use alloc::vec::Vec;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() as isize - 1
    }
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    // add need
    if let Some(task) = current_task() {
        let mut task_inner = task.inner_exclusive_access();
        let entry = task_inner.need_mutex.entry(mutex_id).or_insert(0);
        *entry += 1;
    }
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    // do deadlock detection
    let safe: bool = if process_inner.deadlock_detect_enable {
        let mut finish = process_inner
            .tasks
            .iter()
            .map(|task| task.is_none())
            .collect::<Vec<bool>>();
        let mut work = process_inner
            .mutex_list
            .iter()
            .map(|mutex| {
                if let Some(mutex) = mutex {
                    if mutex.is_locked() {
                        0
                    } else {
                        1
                    }
                } else {
                    0
                }
            })
            .collect::<Vec<usize>>();
        loop {
            let task = process_inner.tasks.iter().find(|task| {
                if let Some(task) = task {
                    let mut task_inner = task.inner_exclusive_access();
                    let tid = task_inner.res.as_ref().unwrap().tid;
                    let need_entry = task_inner.need_mutex.entry(mutex_id).or_insert(0);
                    if !finish[tid] && *need_entry <= work[mutex_id] {
                        finish[task_inner.res.as_ref().unwrap().tid] = true;
                        let allocd_entry = task_inner.alloced_mutex.entry(mutex_id).or_insert(0);
                        work[mutex_id] += *allocd_entry;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            });
            if task.is_none() {
                break finish.iter().all(|ele| *ele);
            }
        }
    } else {
        true
    };
    if !safe {
        if let Some(task) = current_task() {
            let mut task_inner = task.inner_exclusive_access();
            let entry = task_inner.need_mutex.entry(mutex_id).or_insert(0);
            *entry -= 1;
        }
        -0xDEAD
    } else {
        let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
        drop(process_inner);
        drop(process);
        mutex.lock();
        if let Some(task) = current_task() {
            let mut task_inner = task.inner_exclusive_access();
            let need_entry = task_inner.need_mutex.entry(mutex_id).or_insert(0);
            *need_entry -= 1;
            let alloced_entry = task_inner.alloced_mutex.entry(mutex_id).or_insert(0);
            *alloced_entry += 1;
        }
        0
    }
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.unlock();
    if let Some(task) = current_task() {
        let mut task_inner = task.inner_exclusive_access();
        let alloced_entry = task_inner.alloced_mutex.entry(mutex_id).or_insert(0);
        *alloced_entry -= 1;
    }
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        process_inner.semaphore_max[id] = res_count;
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.semaphore_max.push(res_count);
        process_inner.semaphore_list.len() - 1
    };
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    debug!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up: {}",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid,
        sem_id
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    sem.up();
    if let Some(task) = current_task() {
        let mut task_inner = task.inner_exclusive_access();
        debug!(
            "dealloc sem: {}, tid: {}, prev: need: {:?}, allo: {:?}",
            sem_id,
            task_inner.res.as_ref().unwrap().tid,
            task_inner.need_semephore,
            task_inner.alloced_semephore
        );
        let alloced_entry = task_inner.alloced_semephore.entry(sem_id).or_insert(0);
        *alloced_entry -= 1;
    }
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    debug!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down: {}",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid,
        sem_id
    );
    // add need
    if let Some(task) = current_task() {
        let mut task_inner = task.inner_exclusive_access();
        let entry = task_inner.need_semephore.entry(sem_id).or_insert(0);
        *entry += 1;
    }
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    // do deadlock detection
    let safe: bool = if process_inner.deadlock_detect_enable {
        let mut finish = process_inner
            .tasks
            .iter()
            .map(|task| {
                task.as_ref()
                    .map(|task| task.inner_exclusive_access().res.is_none())
                    .is_none()
            })
            .collect::<Vec<bool>>();
        // prepare available list
        let mut work = process_inner.semaphore_max.clone();
        process_inner.tasks.iter().for_each(|task| {
            if let Some(task) = task {
                let task_inner = task.inner_exclusive_access();
                task_inner
                    .alloced_semephore
                    .iter()
                    .for_each(|(sem_id, allocated)| {
                        work[*sem_id] -= *allocated;
                    });
            }
        });
        debug!("work: {:?}", work);
        debug!("finish: {:?}", finish);
        // check is safe
        loop {
            let task = process_inner.tasks.iter().enumerate().find(|(tid, task)| {
                if let Some(task) = task {
                    let task_inner = task.inner_exclusive_access();
                    // let tid = if let Some(res) = task_inner.res.as_ref() {
                    //     res.tid
                    // } else {
                    //     return false;
                    // };
                    let tid = *tid;
                    debug!(
                        "tid: {}, need: {:?}, allo: {:?}",
                        tid, task_inner.need_semephore, task_inner.alloced_semephore
                    );
                    if !finish[tid]
                        && task_inner
                            .need_semephore
                            .iter()
                            .all(|(sem_id, need_num)| *need_num <= work[*sem_id])
                    {
                        task_inner
                            .alloced_semephore
                            .iter()
                            .for_each(|(sem_id, num)| {
                                work[*sem_id] += *num;
                            });
                        finish[tid] = true;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            });
            debug!("work2: {:?}", work);
            debug!("finish2: {:?}", finish);
            if task.is_none() {
                break finish.iter().all(|ele| *ele);
            }
        }
    } else {
        true
    };
    if !safe {
        if let Some(task) = current_task() {
            let mut task_inner = task.inner_exclusive_access();
            let entry = task_inner.need_semephore.entry(sem_id).or_insert(0);
            *entry -= 1;
        }
        debug!(
            "kernel:pid[{}] tid[{}] sem deadlock detected",
            current_task().unwrap().process.upgrade().unwrap().getpid(),
            current_task()
                .unwrap()
                .inner_exclusive_access()
                .res
                .as_ref()
                .unwrap()
                .tid
        );
        -0xDEAD
    } else {
        let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
        drop(process_inner);
        sem.down();
        if let Some(task) = current_task() {
            let mut task_inner = task.inner_exclusive_access();
            debug!(
                "alloc sem: {}, tid: {}, prev: need: {:?}, allo: {:?}",
                sem_id,
                task_inner.res.as_ref().unwrap().tid,
                task_inner.need_semephore,
                task_inner.alloced_semephore
            );
            let need_entry = task_inner.need_semephore.entry(sem_id).or_insert(0);
            *need_entry -= 1;
            let alloced_entry = task_inner.alloced_semephore.entry(sem_id).or_insert(0);
            *alloced_entry += 1;
        }
        0
    }
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
///
/// YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect NOT IMPLEMENTED");
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.deadlock_detect_enable = enabled != 0;
    0
}
