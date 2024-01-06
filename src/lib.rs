use godot::prelude::*;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::panic;
use std::sync::Mutex;
use backtrace::Backtrace;
use regex::Regex;

lazy_static::lazy_static! {
    pub static ref REGISTERED_TESTS: Mutex<VecDeque<fn(&Gd<Node>)>> = Mutex::new(VecDeque::new());
    pub static ref FOCUSED_TEST: Mutex<Option<fn(&Gd<Node>)>> = Mutex::new(None);
    pub static ref CURRENT_TEST_INDEX: Mutex<usize> = Mutex::new(0);
    pub static ref CURRENT_TEST_ITERATION: Mutex<usize> = Mutex::new(0);
    pub static ref WANTS_REPLAY: Mutex<bool> = Mutex::new(false);
    pub static ref DELAY_BEFORE_NEXT_TEST_RUN: Mutex<f64> = Mutex::new(0.0);
}

#[macro_export]
macro_rules! focus {
    ($test_func:ident) => {{
        let mut focused_test = godot_rust_specs::FOCUSED_TEST.lock().unwrap();
        *focused_test = Some($test_func);
    }};
}

#[macro_export]
macro_rules! tick {
    () => {
        godot_rust_specs::CURRENT_TEST_ITERATION
            .lock()
            .unwrap()
            .clone()
    };
}

#[macro_export]
macro_rules! wait {
    ($millis:expr) => {{
        *godot_rust_specs::WANTS_REPLAY.lock().unwrap() = true;
        *godot_rust_specs::DELAY_BEFORE_NEXT_TEST_RUN.lock().unwrap() = $millis.into();
        return;
    }};
}

#[macro_export]
macro_rules! test {
    ($test_func:ident) => {{
        let mut tests = godot_rust_specs::REGISTERED_TESTS.lock().unwrap();
        tests.push_back($test_func);
    }};
}

#[macro_export]
macro_rules! print_red {
    ($($arg:tt)*) => ({
        print!("\x1B[31m");
        print!($($arg)*);
        print!("\x1B[0m");
        io::stdout().flush().unwrap();
    });
}

#[macro_export]
macro_rules! print_green {
    ($($arg:tt)*) => ({
        print!("\x1B[32m");
        print!($($arg)*);
        print!("\x1B[0m");
        io::stdout().flush().unwrap();
    });
}

#[macro_export]
macro_rules! println_red {
    ($($arg:tt)*) => ({
        print!("\x1B[31m");
        print!($($arg)*);
        print!("\x1B[0m\n");
    });
}

#[macro_export]
macro_rules! println_green {
    ($($arg:tt)*) => ({
        print!("\x1B[32m");
        print!($($arg)*);
        print!("\x1B[0m\n");
    });
}

#[macro_export]
macro_rules! println_blue {
    ($($arg:tt)*) => ({
        print!("\x1B[34m");
        print!($($arg)*);
        print!("\x1B[0m\n");
    });
}

#[macro_export]
macro_rules! assert_approx_eq {
    ($a:expr, $b:expr, $epsilon:expr) => {
        if ($a - $b).abs() > $epsilon {
            panic!(
                "assertion failed: ({} - {}).abs() <= {}. Values: {} and {}",
                stringify!($a),
                stringify!($b),
                stringify!($epsilon),
                $a,
                $b
            );
        }
    };
}

#[derive(GodotClass)]
#[class(base=Node)]
struct TestRunner {
    #[base]
    base: Base<Node>,
    time_counter: f64,
    passes: usize,
    failures: usize,
}

#[godot_api]
impl INode for TestRunner {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            time_counter: 0.0,
            passes: 0,
            failures: 0,
        }
    }

    fn ready(&mut self) {
        println!("");

        panic::set_hook(Box::new(|info| {
            println_red!("{}", info);
            let backtrace = Backtrace::new();
            let backtrace = format!("{:?}", backtrace);

            let re = Regex::new(
                r"(?s)\w+\d+: godot_rust_specs::impl\$0::run_test.*"
            ).unwrap();
            let backtrace = re.replace_all(&backtrace, "");

            let re = Regex::new(
                r"(?s)\w+\d+: godot_rust_specs::TestRunner::run_test::.*"
            ).unwrap();
            let backtrace = re.replace_all(&backtrace, "");

            let re = Regex::new(
                r"(?s).*/library\\core\\src\\panicking.rs:\d+"
            ).unwrap();
            let backtrace = re.replace_all(&backtrace, "");

            let re = Regex::new(
                r"(?s).*/library/core/src/panicking.rs:\d+:\d+"
            ).unwrap();
            let backtrace = re.replace_all(&backtrace, "");

            println_blue!("{}", backtrace);
        }));
    }

    fn process(&mut self, delta: f64) {
        self.time_counter += delta;

        let delay = DELAY_BEFORE_NEXT_TEST_RUN.lock().unwrap().clone();

        if self.time_counter > delay {
            self.time_counter = 0.0;
            self.run_test();
        }
    }
}

impl TestRunner {
    fn quit(&mut self) {
        let passes = self.passes;
        let failures = self.failures;
        let total = passes + failures;

        if failures > 0 {
            println_red!("\n\n{} examples, {} failures", total, failures);
        } else {
            println_green!("\n\n{} examples, 0 failures", passes);
        }


        self.base().get_tree().unwrap().quit();
    }

    // Remove all node's childen between tests
    fn cleanup(&mut self) {
        let mut value = CURRENT_TEST_ITERATION.lock().unwrap();
        *value = 0;

        let mut value = DELAY_BEFORE_NEXT_TEST_RUN.lock().unwrap();
        *value = 0.0;

        let mut value = WANTS_REPLAY.lock().unwrap();
        *value = false;

        let mut value = FOCUSED_TEST.lock().unwrap();
        *value = None;

        let children = self.base().get_children();
        for child in children.iter_shared() {
            child.free();
        }
    }

    fn run_test(&mut self) {
        let focus = FOCUSED_TEST.lock().unwrap().clone();
        let tests = crate::REGISTERED_TESTS.lock().unwrap();

        let current_test: Option<&fn(&Gd<Node>)>;

        if focus.is_some() {
            current_test = focus.as_ref();
        } else {
            let current_test_index = CURRENT_TEST_INDEX.lock().unwrap().clone();
            current_test = tests.iter().nth(current_test_index);
        }

        if current_test.is_none() {
            self.quit();
            return;
        }

        let result = panic::catch_unwind(|| {
            current_test.unwrap()(&self.base());
        });

        match result {
            Ok(_) => {
                if WANTS_REPLAY.lock().unwrap().clone() {
                    let mut value = WANTS_REPLAY.lock().unwrap();
                    *value = false;

                    let mut value = CURRENT_TEST_ITERATION.lock().unwrap();
                    *value += 1;

                    return;
                } else {
                    self.passes += 1;
                    print_green!(".");
                }
            }
            Err(_error) => {
                self.failures += 1;
                print_red!("F");
            }
        }

        let mut value = CURRENT_TEST_INDEX.lock().unwrap();
        *value += 1;
        self.cleanup();

        if focus.is_some() {
            self.quit();
            return;
        }
    }
}
