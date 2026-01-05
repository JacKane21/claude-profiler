mod help;
mod profile_list;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::borrow::Cow;

use crate::app::{
    App, AppMode, EDIT_FIELD_API_KEY, EDIT_FIELD_DESCRIPTION, EDIT_FIELD_HAIKU, EDIT_FIELD_NAME,
    EDIT_FIELD_OPUS, EDIT_FIELD_SONNET, EDIT_FIELD_URL,
};
use crate::config::{
    ENV_AUTH_TOKEN, ENV_BASE_URL, ENV_DEFAULT_HAIKU_MODEL, ENV_DEFAULT_OPUS_MODEL,
    ENV_DEFAULT_SONNET_MODEL,
};

pub use help::render_help_popup;
pub use profile_list::render_profile_list;

/// Main UI rendering function
pub fn render(frame: &mut Frame, app: &mut App) {
    let title_height = title_height_for_width(frame.area().width, frame.area().height);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(title_height), // Title
            Constraint::Min(4),    // Profile list (reduced from 8 to allow more room)
            Constraint::Length(8), // Details panel (slightly increased to ensure it fits its title/content)
            Constraint::Length(2), // Footer
        ])
        .split(frame.area());

    render_title(frame, chunks[0]);
    match &app.mode {
        AppMode::LMStudioModelSelection => {
            render_lmstudio_model_list(frame, app, chunks[1]);
        }
        _ => {
            render_profile_list(frame, app, chunks[1]);
        }
    }
    render_details(frame, app, chunks[2]);
    render_footer(frame, chunks[3], app);

    // Overlay help if in help mode
    if app.mode == AppMode::Help {
        let area = centered_rect(60, 50, frame.area());
        render_help_popup(frame, area);
    }

    // Overlay edit form if in edit mode
    if let AppMode::EditProfile {
        focused_field,
        is_creating: _,
    } = app.mode
    {
        let area = centered_rect(70, 80, frame.area());
        render_edit_profile(frame, app, area, focused_field);
    }
}

fn title_height_for_width(_w: u16, _h: u16) -> u16 {
    15 // 6 CLAUDE with shadow + 1 spacer + 6 PROFILER with shadow + 1 spacer + 1 version
}

fn render_title(frame: &mut Frame, area: Rect) {
    // Block art with integrated shadow using box-drawing characters (cfonts style)
    // Main blocks use █, shadow/depth uses ░ positioned to create 3D effect
    let claude = [
        "░█████╗░██╗░░░░░░█████╗░██╗░░░██╗██████╗░███████╗",
        "██╔══██╗██║░░░░░██╔══██╗██║░░░██║██╔══██╗██╔════╝",
        "██║░░╚═╝██║░░░░░███████║██║░░░██║██║░░██║█████╗░░",
        "██║░░██╗██║░░░░░██╔══██║██║░░░██║██║░░██║██╔══╝░░",
        "╚█████╔╝███████╗██║░░██║╚██████╔╝██████╔╝███████╗",
        "░╚════╝░╚══════╝╚═╝░░╚═╝░╚═════╝░╚═════╝░╚══════╝",
    ];

    let profiler = [
        "██████╗░██████╗░░█████╗░███████╗██╗██╗░░░░░███████╗██████╗░",
        "██╔══██╗██╔══██╗██╔══██╗██╔════╝██║██║░░░░░██╔════╝██╔══██╗",
        "██████╔╝██████╔╝██║░░██║█████╗░░██║██║░░░░░█████╗░░██████╔╝",
        "██╔═══╝░██╔══██╗██║░░██║██╔══╝░░██║██║░░░░░██╔══╝░░██╔══██╗",
        "██║░░░░░██║░░██║╚█████╔╝██║░░░░░██║███████╗███████╗██║░░██║",
        "╚═╝░░░░░╚═╝░░╚═╝░╚════╝░╚═╝░░░░░╚═╝╚══════╝╚══════╝╚═╝░░╚═╝",
    ];

    // Light blue to dark blue gradient
    let start = (135u8, 206u8, 250u8); // Light sky blue
    let end = (0u8, 51u8, 153u8); // Dark blue

    fn gradient_line(text: &str, start: (u8, u8, u8), end: (u8, u8, u8)) -> Line<'static> {
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len().max(1) as f32;
        let spans: Vec<Span<'static>> = chars
            .into_iter()
            .enumerate()
            .map(|(i, ch)| {
                let t = if n <= 1.0 { 0.0 } else { i as f32 / (n - 1.0) };
                let r = (start.0 as f32 + (end.0 as f32 - start.0 as f32) * t).round() as u8;
                let g = (start.1 as f32 + (end.1 as f32 - start.1 as f32) * t).round() as u8;
                let b = (start.2 as f32 + (end.2 as f32 - start.2 as f32) * t).round() as u8;
                Span::styled(ch.to_string(), Style::default().fg(Color::Rgb(r, g, b)))
            })
            .collect();
        Line::from(spans)
    }

    let mut result: Vec<Line<'static>> = Vec::new();

    // CLAUDE
    for line in &claude {
        result.push(gradient_line(line, start, end));
    }

    // Spacer
    result.push(Line::from(""));

    // PROFILER
    for line in &profiler {
        result.push(gradient_line(line, start, end));
    }

    // Spacer and version
    result.push(Line::from(""));
    result.push(Line::from(Span::styled(
        "v0.1.0",
        Style::default().fg(Color::DarkGray),
    )));

    let title = Paragraph::new(result)
        .alignment(ratatui::layout::Alignment::Center)
        .block(Block::default());
    frame.render_widget(title, area);
}

fn render_details(frame: &mut Frame, app: &App, area: Rect) {
    let content = if let Some(profile) = app.current_profile() {
        if profile.env.is_empty() {
            vec![Line::from(Span::styled(
                "No environment variables (uses existing environment)",
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            let mut env_items: Vec<(&String, &String)> = profile.env.iter().collect();
            env_items.sort_by(|(a, _), (b, _)| a.cmp(b));
            env_items
                .into_iter()
                .map(|(key, value)| {
                    let display_value = if is_sensitive_key(key) {
                        mask_value(value)
                    } else {
                        value.to_string()
                    };
                    Line::from(vec![
                        Span::styled(key.as_str(), Style::default().fg(Color::Yellow)),
                        Span::raw(" = "),
                        Span::styled(
                            format!("\"{}\"", display_value),
                            Style::default().fg(Color::Green),
                        ),
                    ])
                })
                .collect()
        }
    } else {
        vec![Line::from("No profile selected")]
    };

    let details = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::TOP)
            .title("Environment Variables"),
    );
    frame.render_widget(details, area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let footer_text = if let Some(ref msg) = app.status_message {
        let msg_lower = msg.to_ascii_lowercase();
        let is_error = msg_lower.contains("failed") || msg_lower.contains("error");
        let (label, color) = if is_error {
            ("Error: ", Color::Red)
        } else {
            ("Success: ", Color::Green)
        };
        Line::from(vec![
            Span::styled(label, Style::default().fg(color)),
            Span::raw(msg),
            Span::raw(" (press any key to clear)"),
        ])
    } else {
        match app.mode {
            AppMode::LMStudioModelSelection => {
                let mode_label = if app.selecting_auxiliary_model {
                    "AUX"
                } else {
                    "MAIN"
                };
                let mode_color = if app.selecting_auxiliary_model {
                    Color::Yellow
                } else {
                    Color::Cyan
                };
                Line::from(vec![
                    Span::styled("[", Style::default().fg(Color::DarkGray)),
                    Span::styled("^/v", Style::default().fg(Color::Cyan)),
                    Span::styled("] Navigate  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[", Style::default().fg(Color::DarkGray)),
                    Span::styled("a", Style::default().fg(Color::Cyan)),
                    Span::styled("] ", Style::default().fg(Color::DarkGray)),
                    Span::styled(mode_label, Style::default().fg(mode_color)),
                    Span::styled("  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[", Style::default().fg(Color::DarkGray)),
                    Span::styled("Enter", Style::default().fg(Color::Cyan)),
                    Span::styled("] Select  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[", Style::default().fg(Color::DarkGray)),
                    Span::styled("r", Style::default().fg(Color::Cyan)),
                    Span::styled("] Refresh  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[", Style::default().fg(Color::DarkGray)),
                    Span::styled("l", Style::default().fg(Color::Cyan)),
                    Span::styled("] Open App  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[", Style::default().fg(Color::DarkGray)),
                    Span::styled("Esc", Style::default().fg(Color::Cyan)),
                    Span::styled("] Back", Style::default().fg(Color::DarkGray)),
                ])
            }
            _ => Line::from(vec![
                Span::styled("[", Style::default().fg(Color::DarkGray)),
                Span::styled("^/v", Style::default().fg(Color::Cyan)),
                Span::styled("] Navigate  ", Style::default().fg(Color::DarkGray)),
                Span::styled("[", Style::default().fg(Color::DarkGray)),
                Span::styled("Enter", Style::default().fg(Color::Cyan)),
                Span::styled("] Launch  ", Style::default().fg(Color::DarkGray)),
                Span::styled("[", Style::default().fg(Color::DarkGray)),
                Span::styled("?", Style::default().fg(Color::Cyan)),
                Span::styled("] Help  ", Style::default().fg(Color::DarkGray)),
                Span::styled("[", Style::default().fg(Color::DarkGray)),
                Span::styled("e", Style::default().fg(Color::Cyan)),
                Span::styled("] Edit  ", Style::default().fg(Color::DarkGray)),
                Span::styled("[", Style::default().fg(Color::DarkGray)),
                Span::styled("n", Style::default().fg(Color::Cyan)),
                Span::styled("] New  ", Style::default().fg(Color::DarkGray)),
                Span::styled("[", Style::default().fg(Color::DarkGray)),
                Span::styled("d", Style::default().fg(Color::Cyan)),
                Span::styled("] Delete  ", Style::default().fg(Color::DarkGray)),
                Span::styled("[", Style::default().fg(Color::DarkGray)),
                Span::styled("r", Style::default().fg(Color::Cyan)),
                Span::styled("] Reset  ", Style::default().fg(Color::DarkGray)),
                Span::styled("[", Style::default().fg(Color::DarkGray)),
                Span::styled("l", Style::default().fg(Color::Cyan)),
                Span::styled("] LMStudio Models  ", Style::default().fg(Color::DarkGray)),
                Span::styled("[", Style::default().fg(Color::DarkGray)),
                Span::styled("q", Style::default().fg(Color::Cyan)),
                Span::styled("] Quit", Style::default().fg(Color::DarkGray)),
            ]),
        }
    };

    let footer = Paragraph::new(footer_text).block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, area);
}

fn render_edit_profile(frame: &mut Frame, app: &App, area: Rect, focused_field: usize) {
    frame.render_widget(Clear, area);

    let (title, _) = if let AppMode::EditProfile { is_creating, .. } = app.mode {
        (
            if is_creating {
                " Create Profile "
            } else {
                " Edit Profile "
            },
            is_creating,
        )
    } else {
        (" Edit Profile ", false)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(Style::default().bg(Color::Black));
    frame.render_widget(block, area);

    let inner_area = area.inner(ratatui::layout::Margin {
        vertical: 2,
        horizontal: 2,
    });

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Name
            Constraint::Length(3), // Description
            Constraint::Length(3), // API Key
            Constraint::Length(3), // URL
            Constraint::Length(3), // Haiku
            Constraint::Length(3), // Sonnet
            Constraint::Length(3), // Opus
            Constraint::Min(1),    // Spacer
            Constraint::Length(1), // Help
        ])
        .split(inner_area);

    render_edit_field(
        frame,
        chunks[0],
        "Profile Name",
        app.name_input.value(),
        focused_field == EDIT_FIELD_NAME,
    );
    render_edit_field(
        frame,
        chunks[1],
        "Description",
        app.description_input.value(),
        focused_field == EDIT_FIELD_DESCRIPTION,
    );

    let api_key_value: Cow<'_, str> = if app.reveal_api_key {
        Cow::Borrowed(app.api_key_input.value())
    } else {
        Cow::Owned("*".repeat(app.api_key_input.value().len()))
    };

    render_edit_field(
        frame,
        chunks[2],
        ENV_AUTH_TOKEN,
        api_key_value.as_ref(),
        focused_field == EDIT_FIELD_API_KEY,
    );
    render_edit_field(
        frame,
        chunks[3],
        ENV_BASE_URL,
        app.url_input.value(),
        focused_field == EDIT_FIELD_URL,
    );
    render_edit_field(
        frame,
        chunks[4],
        ENV_DEFAULT_HAIKU_MODEL,
        app.haiku_model_input.value(),
        focused_field == EDIT_FIELD_HAIKU,
    );
    render_edit_field(
        frame,
        chunks[5],
        ENV_DEFAULT_SONNET_MODEL,
        app.sonnet_model_input.value(),
        focused_field == EDIT_FIELD_SONNET,
    );
    render_edit_field(
        frame,
        chunks[6],
        ENV_DEFAULT_OPUS_MODEL,
        app.opus_model_input.value(),
        focused_field == EDIT_FIELD_OPUS,
    );

    let help_text = Line::from(vec![
        Span::styled("Tab", Style::default().fg(Color::Cyan)),
        Span::raw(" Switch  "),
        Span::styled("Ctrl+G", Style::default().fg(Color::Cyan)),
        Span::raw(" Toggle Reveal  "),
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::raw(" Save  "),
        Span::styled("Esc", Style::default().fg(Color::Cyan)),
        Span::raw(" Cancel"),
    ]);
    frame.render_widget(Paragraph::new(help_text), chunks[8]);

    // Set cursor
    let cursor_positions = [
        (chunks[0], app.name_input.visual_cursor() as u16),
        (chunks[1], app.description_input.visual_cursor() as u16),
        (chunks[2], app.api_key_input.visual_cursor() as u16),
        (chunks[3], app.url_input.visual_cursor() as u16),
        (chunks[4], app.haiku_model_input.visual_cursor() as u16),
        (chunks[5], app.sonnet_model_input.visual_cursor() as u16),
        (chunks[6], app.opus_model_input.visual_cursor() as u16),
    ];
    if let Some((chunk, cursor_x)) = cursor_positions.get(focused_field) {
        frame.set_cursor_position((chunk.x + *cursor_x + 1, chunk.y + 1));
    }
}

/// Helper function to create a centered rectangle
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn field_border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn render_edit_field(frame: &mut Frame, area: Rect, title: &str, value: &str, focused: bool) {
    let title_line = Line::from(vec![Span::raw(" "), Span::raw(title), Span::raw(" ")]);
    let input = Paragraph::new(value).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title_line)
            .border_style(field_border_style(focused)),
    );
    frame.render_widget(input, area);
}

fn is_sensitive_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper.contains("TOKEN") || upper.contains("KEY") || upper.contains("SECRET")
}

fn mask_value(value: &str) -> String {
    if value.len() > 8 {
        format!("{}...{}", &value[..4], &value[value.len() - 4..])
    } else {
        "****".to_string()
    }
}

fn render_lmstudio_model_list(frame: &mut Frame, app: &mut App, area: Rect) {
    use ratatui::style::Modifier;
    use ratatui::widgets::{List, ListItem};

    // Get current selections for display
    let lmstudio_profile = app.config.profiles.iter().find(|p| p.name == "lmstudio");
    let current_main = lmstudio_profile
        .and_then(|p| p.env.get(crate::config::ENV_MODEL))
        .map(|s| s.as_str())
        .unwrap_or("none");
    let current_aux = lmstudio_profile
        .and_then(|p| p.env.get(crate::config::ENV_SMALL_FAST_MODEL))
        .map(|s| s.as_str())
        .unwrap_or("none");

    let items: Vec<ListItem> = app
        .lmstudio_models
        .iter()
        .map(|model| {
            // Mark current selections
            let mut spans = vec![Span::styled(
                model,
                Style::default().add_modifier(Modifier::BOLD),
            )];

            let is_main = model == current_main;
            let is_aux = model == current_aux;
            if is_main && is_aux {
                spans.push(Span::styled(
                    " [main+aux]",
                    Style::default().fg(Color::Magenta),
                ));
            } else if is_main {
                spans.push(Span::styled(" [main]", Style::default().fg(Color::Cyan)));
            } else if is_aux {
                spans.push(Span::styled(" [aux]", Style::default().fg(Color::Yellow)));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    // Build title based on selection mode
    let title = if app.selecting_auxiliary_model {
        format!(
            "Select AUXILIARY Model (Main: {}) [a] toggle [l] open app",
            current_main
        )
    } else {
        format!(
            "Select MAIN Model (Aux: {}) [a] toggle [l] open app",
            current_aux
        )
    };

    let title_style = if app.selecting_auxiliary_model {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Cyan)
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .title(Span::styled(title, title_style)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(list, area, &mut app.lmstudio_list_state);
}

#[cfg(test)]
mod tests {
    use super::{is_sensitive_key, mask_value};

    #[test]
    fn sensitive_key_detection() {
        assert!(is_sensitive_key("API_KEY"));
        assert!(is_sensitive_key("auth_token"));
        assert!(is_sensitive_key("my_secret"));
        assert!(!is_sensitive_key("model"));
    }

    #[test]
    fn mask_value_short_and_long() {
        assert_eq!(mask_value("short"), "****");
        assert_eq!(mask_value("1234567890"), "1234...7890");
    }
}
