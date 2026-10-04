mod app;
mod backend;
mod error;
mod terminal;

use app::App;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new()?;
    app.run()?;
    Ok(())
}
