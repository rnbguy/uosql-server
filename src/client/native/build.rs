//! Compile the listed C-files

extern crate cc;

fn main() {
    cc::Build::new()
        .file("src/client/native/console_raw.c")
        .compile("librawconsole.a");
}
