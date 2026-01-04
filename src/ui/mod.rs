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
    App, AppMode, EDIT_FIELD_API_KEY, EDIT_FIELD_HAIKU, EDIT_FIELD_OPUS, EDIT_FIELD_SONNET,
    EDIT_FIELD_URL,
};
use crate::config::{
    ENV_AUTH_TOKEN, ENV_BASE_URL, ENV_DEFAULT_HAIKU_MODEL, ENV_DEFAULT_OPUS_MODEL,
    ENV_DEFAULT_SONNET_MODEL,
};

pub use help::render_help_popup;
pub use profile_list::render_profile_list;

/// Main UI rendering function
pub fn render(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Min(8),    // Profile list
            Constraint::Length(7), // Details panel
            Constraint::Length(3), // Footer
        ])
        .split(frame.area());

    render_title(frame, chunks[0]);
    render_profile_list(frame, app, chunks[1]);
    render_details(frame, app, chunks[2]);
    render_footer(frame, chunks[3], app);

    // Overlay help if in help mode
    if app.mode == AppMode::Help {
        let area = centered_rect(60, 50, frame.area());
        render_help_popup(frame, area);
    }

    // Overlay edit form if in edit mode
    if let AppMode::EditProfile { focused_field } = app.mode {
        let area = centered_rect(70, 70, frame.area());
        render_edit_profile(frame, app, area, focused_field);
    }
}

fn render_title(frame: &mut Frame, area: Rect) {
    let title = Paragraph::new(Line::from(vec![
        Span::styled("ClaudeProfiler", Style::default().fg(Color::Cyan)),
        Span::raw(" v0.1.0"),
    ]))
    .block(Block::default().borders(Borders::ALL));
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
            .borders(Borders::ALL)
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
        Line::from(vec![
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
            Span::styled("] Edit Profile  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[", Style::default().fg(Color::DarkGray)),
            Span::styled("r", Style::default().fg(Color::Cyan)),
            Span::styled("] Reset Config  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[", Style::default().fg(Color::DarkGray)),
            Span::styled("q", Style::default().fg(Color::Cyan)),
            Span::styled("] Quit", Style::default().fg(Color::DarkGray)),
        ])
    };

    let footer = Paragraph::new(footer_text).block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, area);
}

fn render_edit_profile(frame: &mut Frame, app: &App, area: Rect, focused_field: usize) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Edit Profile ")
        .style(Style::default().bg(Color::Black));
    frame.render_widget(block, area);

    let inner_area = area.inner(ratatui::layout::Margin {
        vertical: 2,
        horizontal: 2,
    });

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // API Key
            Constraint::Length(3), // URL
            Constraint::Length(3), // Haiku
            Constraint::Length(3), // Sonnet
            Constraint::Length(3), // Opus
            Constraint::Min(1),    // Spacer
            Constraint::Length(1), // Help
        ])
        .split(inner_area);

    let api_key_value: Cow<'_, str> = if app.reveal_api_key {
        Cow::Borrowed(app.api_key_input.value())
    } else {
        Cow::Owned("*".repeat(app.api_key_input.value().len()))
    };

    render_edit_field(
        frame,
        chunks[0],
        ENV_AUTH_TOKEN,
        api_key_value.as_ref(),
        focused_field == EDIT_FIELD_API_KEY,
    );
    render_edit_field(
        frame,
        chunks[1],
        ENV_BASE_URL,
        app.url_input.value(),
        focused_field == EDIT_FIELD_URL,
    );
    render_edit_field(
        frame,
        chunks[2],
        ENV_DEFAULT_HAIKU_MODEL,
        app.haiku_model_input.value(),
        focused_field == EDIT_FIELD_HAIKU,
    );
    render_edit_field(
        frame,
        chunks[3],
        ENV_DEFAULT_SONNET_MODEL,
        app.sonnet_model_input.value(),
        focused_field == EDIT_FIELD_SONNET,
    );
    render_edit_field(
        frame,
        chunks[4],
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
    frame.render_widget(Paragraph::new(help_text), chunks[6]);

    // Set cursor
    let cursor_positions = [
        (chunks[0], app.api_key_input.visual_cursor() as u16),
        (chunks[1], app.url_input.visual_cursor() as u16),
        (chunks[2], app.haiku_model_input.visual_cursor() as u16),
        (chunks[3], app.sonnet_model_input.visual_cursor() as u16),
        (chunks[4], app.opus_model_input.visual_cursor() as u16),
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
