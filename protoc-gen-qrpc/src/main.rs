//! qRPC protoc plugin entrypoint.

mod go_gen;
mod plugin;
mod python_gen;
mod rust_gen;

fn main() {
    if let Err(err) = plugin::run(std::io::stdin().lock(), std::io::stdout().lock()) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
