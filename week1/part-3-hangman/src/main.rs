// Simple Hangman Program
// User gets five incorrect guesses
// Word chosen randomly from words.txt
// Inspiration from: https://doc.rust-lang.org/book/ch02-00-guessing-game-tutorial.html
// This assignment will introduce you to some fundamental syntax in Rust:
// - variable declaration
// - string manipulation
// - conditional statements
// - loops
// - vectors
// - files
// - user input We've tried to limit/hide Rust's quirks since we'll discuss those details
// more in depth in the coming lectures.
extern crate rand;
use rand::Rng;
use std::fs;
use std::io;
use std::io::Write;

const NUM_INCORRECT_GUESSES: u32 = 5;
const WORDS_PATH: &str = "words.txt";

fn pick_a_random_word() -> String {
    let file_string = fs::read_to_string(WORDS_PATH).expect("Unable to read file.");
    let words: Vec<&str> = file_string.split('\n').collect();
    String::from(words[rand::thread_rng().gen_range(0, words.len())].trim())
}

fn main() {
    let secret_word = pick_a_random_word();
    // Note: given what you know about Rust so far, it's easier to pull characters out of a
    // vector than it is to pull them out of a string. You can get the ith character of
    // secret_word by doing secret_word_chars[i].
    let secret_word_chars: Vec<char> = secret_word.chars().collect();
    // Uncomment for debugging:
    //println!("random word: {}", secret_word);

    // Your code here! :)
    let mut guess_string: Vec<i32> = vec![1; secret_word_chars.len()];

    let mut correct_guess = false;
    let mut i = NUM_INCORRECT_GUESSES;

    let mut total: i32 = 100;
    while i > 0 {
        println!("You have {} guesses left", i);

        print!("Give a guess: ");
        io::stdout().flush().expect("Error flushing stdout");

        let mut guess: String = String::new();
        io::stdin()
            .read_line(&mut guess)
            .expect("Error reading line");

        if guess.len() as i32 != 2 {
            println!("Please input only 1 character");
            continue;
        }

        for (i, c) in secret_word.chars().enumerate() {
            //println!("guess: {}, char: {}",guess.trim(), c.to_string()==guess);
            if guess.trim() == c.to_string() && guess_string[i] == 1 {
                guess_string[i] = 0;
                correct_guess = true;
                println!("You guess {} correctly as position {}!", c, i + 1);
                break;
            }
        }

        i -= 1;
        if correct_guess {
            correct_guess = false;
            i += 1;
        }
        let mut sofar: String = String::with_capacity(secret_word.len());
        for (i, guessed) in guess_string.iter().enumerate() {
            if *guessed == 0 {
                sofar.push(secret_word.chars().nth(i).unwrap());
            } else {
                sofar.push_str("-");
            }
        }
        println!("Guessed so far {}", sofar);
        total = guess_string.iter().sum();
        if total == 0 {
            break;
        }
    }

    if total == 0 {
        println!("You guessed the whole word correctly");
    } else {
        println!("You weren't able to get it this time :(");
    }
}
