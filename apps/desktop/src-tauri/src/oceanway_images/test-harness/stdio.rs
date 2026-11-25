//! Native process fixture exercising the same early entry point as the app.
fn main() {
    match oceanway_image_service_tests::oceanway_images::run_stdio_if_requested() {
        Some(Ok(())) => {}
        Some(Err(message)) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
        None => std::process::exit(2),
    }
}
