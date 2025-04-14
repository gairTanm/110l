use std::collections::VecDeque;
#[allow(unused_imports)]
use std::sync::{Arc, Mutex};
use std::time::Instant;
#[allow(unused_imports)]
use std::{env, process, thread};

/// Determines whether a number is prime. This function is taken from CS 110 factor.py.
///
/// You don't need to read or understand this code.
#[allow(dead_code)]
fn is_prime(num: u32) -> bool {
    if num <= 1 {
        return false;
    }
    for factor in 2..((num as f64).sqrt().floor() as u32) {
        if num % factor == 0 {
            return false;
        }
    }
    true
}

/// Determines the prime factors of a number and prints them to stdout. This function is taken
/// from CS 110 factor.py.
///
/// You don't need to read or understand this code.
#[allow(dead_code)]
fn factor_number(num: u32) {
    let start = Instant::now();

    if num == 1 || is_prime(num) {
        println!("{} = {} [time: {:?}]", num, num, start.elapsed());
        return;
    }

    let mut factors = Vec::new();
    let mut curr_num = num;
    for factor in 2..num {
        while curr_num % factor == 0 {
            factors.push(factor);
            curr_num /= factor;
        }
    }
    factors.sort();
    let factors_str = factors
        .into_iter()
        .map(|f| f.to_string())
        .collect::<Vec<String>>()
        .join(" * ");
    println!("{} = {} [time: {:?}]", num, factors_str, start.elapsed());
}

/// Returns a list of numbers supplied via argv.
#[allow(dead_code)]
fn get_input_numbers() -> VecDeque<u32> {
    let mut numbers = VecDeque::new();
    for arg in env::args().skip(1) {
        if let Ok(val) = arg.parse::<u32>() {
            numbers.push_back(val);
        } else {
            println!("{} is not a valid number", arg);
            process::exit(1);
        }
    }
    numbers
}

fn threaded_factor(num_threads: usize, numbers: VecDeque<u32>) {
    let start = Instant::now();
    let numbers_mutex = Arc::new(Mutex::new(numbers));
    let mut threads = Vec::with_capacity(num_threads);
    // TODO: spawn `num_threads` threads, each of which pops numbers off the queue and calls
    // factor_number() until the queue is empty
    for _ in 0..num_threads {
        let mut_clone = Arc::clone(&numbers_mutex);
        //let numbers_mutex_clone = Arc::clone(&numbers_mutex);
        threads.push(thread::spawn(move || loop {
            let _result = {
                let mut num = mut_clone.lock().unwrap();
                num.pop_back()
            };

            match _result {
                Some(val) => factor_number(val),
                None => break,
            }
        }))
    }

    // TODO: join all the threads you created

    for thread in threads {
        thread.join().unwrap()
    }
    println!("Total execution time: {:?}", start.elapsed());
}

fn main() {
    let num_threads = num_cpus::get();
    let single_thread = 1;
    println!("Farm starting on {} CPUs", num_threads);

    // TODO: call get_input_numbers() and store a queue of numbers to factor
    let numbers = get_input_numbers();
    let numbers_single = numbers.clone();

    threaded_factor(num_threads, numbers);
    threaded_factor(single_thread, numbers_single);
}
