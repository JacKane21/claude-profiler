mod app;
mod config;
mod launcher;
mod proxy;
mod tui;
mod ui;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::time::Duration;

use crate::app::{
    Action, App, AppMode, EDIT_FIELD_API_KEY, EDIT_FIELD_COUNT, EDIT_FIELD_DESCRIPTION,
    EDIT_FIELD_HAIKU, EDIT_FIELD_NAME, EDIT_FIELD_OPUS, EDIT_FIELD_PROXY_URL, EDIT_FIELD_SONNET,
    EDIT_FIELD_URL,
};
use crate::config::{Config, Profile};
use tui_input::backend::crossterm::EventHandler;

fn main() -> Result<()> {
    // Install panic hook for clean terminal restoration
    tui::install_panic_hook();

    // Load or create config
    let config = Config::load()?;

    if config.profiles.is_empty() {
        eprintln!("No profiles defined in configuration.");
        eprintln!(
            "Please add profiles to: {}",
            Config::config_file_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "~/.config/claude-profiler/profiles.toml".to_string())
        );
        return Ok(());
    }

    // Initialize app state
    let mut app = App::new(config);

    // Initialize terminal
    let mut terminal = tui::init()?;

    // Main event loop
    let result = run_app(&mut terminal, &mut app);

    // Restore the terminal before potentially launching Claude
    tui::restore()?;

    // Handle the result
    match result {
        Ok(Some(profile)) => {
            // User selected a profile - launch Claude Code
            println!("Launching Claude Code with profile: {}", profile.name);
            launcher::exec_claude(&profile)?;
        }
        Ok(None) => {
            // User quit without selecting
            println!("Goodbye!");
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

const UI_POLL_GRANULARITY: Duration = Duration::from_millis(250);

fn run_app(terminal: &mut tui::Tui, app: &mut App) -> Result<Option<Profile>> {
    loop {
        // Render
        terminal.draw(|frame| ui::render(frame, app))?;

        // Poll for events with a timeout (also enables periodic refresh while idle)
        if !event::poll(UI_POLL_GRANULARITY)? {
            continue;
        }

        // Handle input
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            // Clear status message on any key press in Normal mode
            if app.mode == AppMode::Normal && app.status_message.is_some() {
                app.status_message = None;
                continue;
            }

            let action = match app.mode {
                AppMode::Normal => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
                    KeyCode::Up | KeyCode::Char('k') => Some(Action::MoveUp),
                    KeyCode::Down | KeyCode::Char('j') => Some(Action::MoveDown),
                    KeyCode::Enter => Some(Action::SelectProfile),
                    KeyCode::Char('?') => Some(Action::ShowHelp),
                    KeyCode::Char('e') => Some(Action::EditProfile),
                    KeyCode::Char('n') => Some(Action::CreateProfile),
                    KeyCode::Char('r') => Some(Action::ResetProfile),
                    KeyCode::Char('R') => Some(Action::ResetAll),
                    KeyCode::Char('d') => Some(Action::DeleteProfile),
                    _ => None,
                },
                AppMode::Help => Some(Action::HideHelp),
                AppMode::EditProfile {
                    focused_field,
                    is_creating,
                } => match key.code {
                    KeyCode::Esc => Some(Action::CancelEdit),
                    KeyCode::Enter => Some(Action::SaveEdit),
                    KeyCode::Tab | KeyCode::Down => {
                        app.mode = AppMode::EditProfile {
                            focused_field: (focused_field + 1) % EDIT_FIELD_COUNT,
                            is_creating,
                        };
                        None
                    }
                    KeyCode::BackTab | KeyCode::Up => {
                        app.mode = AppMode::EditProfile {
                            focused_field: focused_field
                                .checked_sub(1)
                                .unwrap_or(EDIT_FIELD_COUNT - 1),
                            is_creating,
                        };
                        None
                    }
                    KeyCode::Char('g')
                        if key.modifiers.contains(event::KeyModifiers::CONTROL)
                            && focused_field == EDIT_FIELD_API_KEY =>
                    {
                        app.reveal_api_key = !app.reveal_api_key;
                        None
                    }
                    _ => {
                        handle_edit_input(app, focused_field, key);
                        None
                    }
                },
            };

            if let Some(action) = action {
                app.handle_action(action);
            }

            if app.should_quit {
                return Ok(None);
            }

            if let Some(profile) = app.selected_profile.take() {
                return Ok(Some(profile));
            }
        }
    }
}

fn handle_edit_input(app: &mut App, focused_field: usize, key: event::KeyEvent) {
    match focused_field {
        EDIT_FIELD_NAME => app.name_input.handle_event(&Event::Key(key)),
        EDIT_FIELD_DESCRIPTION => app.description_input.handle_event(&Event::Key(key)),
        EDIT_FIELD_API_KEY => app.api_key_input.handle_event(&Event::Key(key)),
        EDIT_FIELD_URL => app.url_input.handle_event(&Event::Key(key)),
        EDIT_FIELD_PROXY_URL => app.proxy_url_input.handle_event(&Event::Key(key)),
        EDIT_FIELD_HAIKU => app.haiku_model_input.handle_event(&Event::Key(key)),
        EDIT_FIELD_SONNET => app.sonnet_model_input.handle_event(&Event::Key(key)),
        EDIT_FIELD_OPUS => app.opus_model_input.handle_event(&Event::Key(key)),
        _ => None,
    };
}
