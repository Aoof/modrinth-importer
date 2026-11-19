fn main() {
    println!(
        "cargo:rustc-env=MRIMPORT_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
}
