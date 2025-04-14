use std::fs::File;
use std::io::BufRead;
use std::process;
use std::{env, io};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Too few arguments.");
        process::exit(1);
    }
    let filename = &args[1];
    // Your code here :)

    let file = File::open(filename).unwrap();

    let mut line_count = 0;
    let mut word_count = 0;
    let mut char_count = 0;
    for line in io::BufReader::new(file).lines() {
        let line_str = line.unwrap();

        char_count += line_str.len() as i32;

        for word in line_str.split([' ']) {
            if word.len() == 0 {
                continue;
            }
            print!("{word} ");
            word_count += 1;
        }
        line_count += 1;
    }

    println!("In {filename}: Lines: {line_count} \t Words: {word_count} \t Chars: {char_count}")
}
