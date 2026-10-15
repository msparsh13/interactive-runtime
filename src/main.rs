mod app;
mod backend;
mod constants;
mod error;
mod pane;
mod terminal;

use app::App;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new()?;
    app.run()?;
    Ok(())
}
