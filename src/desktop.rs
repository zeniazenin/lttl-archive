//! Desktop entry point: Windows never allocates a console for this executable.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use clap::{Parser, Subcommand};
use std::path::PathBuf;
mod gui;

#[derive(Parser)]
#[command(version, about = "lttl desktop archiver")]
struct Desktop {
    #[command(subcommand)]
    command: Option<Action>,
}

#[derive(Subcommand)]
enum Action {
    Gui { archive: Option<PathBuf> },
    GuiCreate { path: PathBuf },
    GuiExtractHere { path: PathBuf },
    GuiExtractFolder { path: PathBuf },
}

fn main() {
    let result = Desktop::try_parse().map_err(anyhow::Error::from).and_then(|args| {
        let action = match args.command {
            Some(Action::Gui { archive: Some(path) }) => Some(gui::GuiAction::Open(path)),
            Some(Action::GuiCreate { path }) => Some(gui::GuiAction::Create(path)),
            Some(Action::GuiExtractHere { path }) => Some(gui::GuiAction::ExtractHere(path)),
            Some(Action::GuiExtractFolder { path }) => Some(gui::GuiAction::ExtractFolder(path)),
            _ => None,
        };
        gui::run(action)
    });
    if let Err(error) = result {
        rfd::MessageDialog::new().set_title("lttl")
            .set_description(format!("{error:#}"))
            .set_level(rfd::MessageLevel::Error).show();
        std::process::exit(1);
    }
}
