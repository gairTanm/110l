use crossbeam_channel::{unbounded, Receiver, Sender};
use std::iter;
use std::{thread, time};

struct MapResult<T> {
    result: T,
    idx: usize,
}

fn parallel_map<T, U, F>(mut input_vec: Vec<T>, num_threads: usize, f: F) -> Vec<U>
where
    F: FnOnce(T) -> U + Send + Copy + 'static,
    T: Send + 'static,
    U: Send + 'static + Default,
{
    let mut output_vec: Vec<U> = iter::repeat_with(U::default)
        .take(input_vec.len())
        .collect();
    let (sender, receiver)/*: (Sender<MapResult<T>>, Receiver<MapResult<T>>) */= unbounded();
    let (res_sender, res_receiver) = unbounded();

    for (idx, input) in input_vec.into_iter().enumerate() {
        sender.send((input, idx)).unwrap();
    }
    let mut receivers = Vec::new();
    for _ in 0..num_threads {
        let receiver: Receiver<(T, usize)> = receiver.clone();
        let res_sender: Sender<(U, usize)> = res_sender.clone();
        receivers.push(thread::spawn(move || {
            while let Ok((next_num, idx)) = receiver.recv() {
                let res = f(next_num);
                res_sender.send((res, idx)).unwrap();
            }
        }))
    }

    drop(sender);
    drop(res_sender);
    for receiver in receivers {
        receiver.join().expect("not joined");
    }

    while let Ok((res, idx)) = res_receiver.recv() {
        output_vec[idx] = res;
    }
    drop(res_receiver);
    println!("{}", output_vec.len());
    output_vec
}

fn main() {
    let v = vec![6, 7, 8, 9, 10, 1, 2, 3, 4, 5, 12, 18, 11, 5, 20];
    let squares = parallel_map(v, 10, |num| {
        println!("{} squared is {}", num, num * num);
        thread::sleep(time::Duration::from_millis(500));
        num * num
    });
    println!("squares: {:?}", squares);
}
