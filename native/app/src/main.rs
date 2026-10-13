fn main() {
    if let Err(error) = desktop_pet_core::run() {
        eprintln!("desktop-pet: {error}");
        std::process::exit(1);
    }
}
