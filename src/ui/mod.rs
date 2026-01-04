mod help;
mod profile_list;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::{App, AppMode};

pub use help::render_help_popup;
pub use profile_list::render_profile_list;

/// Main UI rendering function
pub fn render(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(8),     // Profile list
            Constraint::Length(7),  // Details panel
            Constraint::Length(3),  // Footer
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
        Span::styled(
            "ClaudeProfiler",
            Style::default().fg(Color::Cyan),
        ),
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
            let mut lines: Vec<Line> = profile
                .env
                .iter()
                .map(|(key, value)| {
                    let display_value = if key.to_uppercase().contains("TOKEN")
                        || key.to_uppercase().contains("KEY")
                        || key.to_uppercase().contains("SECRET")
                    {
                        // Mask sensitive values
                        if value.len() > 8 {
                            format!("{}...{}", &value[..4], &value[value.len() - 4..])
                        } else {
                            "****".to_string()
                        }
                    } else {
                        value.clone()
                    };
                    Line::from(vec![
                        Span::styled(key.clone(), Style::default().fg(Color::Yellow)),
                        Span::raw(" = "),
                        Span::styled(format!("\"{}\"", display_value), Style::default().fg(Color::Green)),
                    ])
                })
                .collect();
            lines.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
            lines
        }
    } else {
        vec![Line::from("No profile selected")]
    };

    let details = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL).title("Environment Variables"));
    frame.render_widget(details, area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let footer_text = if let Some(ref msg) = app.status_message {
        let is_error = msg.to_lowercase().contains("failed") || msg.to_lowercase().contains("error");
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

    let footer = Paragraph::new(footer_text)
        .block(Block::default().borders(Borders::ALL));
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

    let api_key_style = if focused_field == 0 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let api_key_value = if app.reveal_api_key {
        app.api_key_input.value().to_string()
    } else {
        "*".repeat(app.api_key_input.value().len())
    };

    let api_key_input = Paragraph::new(api_key_value).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" ANTHROPIC_AUTH_TOKEN ")
            .border_style(api_key_style),
    );
    frame.render_widget(api_key_input, chunks[0]);

    let url_style = if focused_field == 1 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let url_input = Paragraph::new(app.url_input.value()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" ANTHROPIC_BASE_URL ")
            .border_style(url_style),
    );
    frame.render_widget(url_input, chunks[1]);

    let haiku_style = if focused_field == 2 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let haiku_input = Paragraph::new(app.haiku_model_input.value()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" ANTHROPIC_DEFAULT_HAIKU_MODEL ")
            .border_style(haiku_style),
    );
    frame.render_widget(haiku_input, chunks[2]);

    let sonnet_style = if focused_field == 3 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let sonnet_input = Paragraph::new(app.sonnet_model_input.value()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" ANTHROPIC_DEFAULT_SONNET_MODEL ")
            .border_style(sonnet_style),
    );
    frame.render_widget(sonnet_input, chunks[3]);

    let opus_style = if focused_field == 4 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let opus_input = Paragraph::new(app.opus_model_input.value()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" ANTHROPIC_DEFAULT_OPUS_MODEL ")
            .border_style(opus_style),
    );
    frame.render_widget(opus_input, chunks[4]);

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
    let cursor_pos = match focused_field {
        0 => (
            chunks[0].x + app.api_key_input.visual_cursor() as u16 + 1,
            chunks[0].y + 1,
        ),
        1 => (
            chunks[1].x + app.url_input.visual_cursor() as u16 + 1,
            chunks[1].y + 1,
        ),
        2 => (
            chunks[2].x + app.haiku_model_input.visual_cursor() as u16 + 1,
            chunks[2].y + 1,
        ),
        3 => (
            chunks[3].x + app.sonnet_model_input.visual_cursor() as u16 + 1,
            chunks[3].y + 1,
        ),
        4 => (
            chunks[4].x + app.opus_model_input.visual_cursor() as u16 + 1,
            chunks[4].y + 1,
        ),
        _ => (0, 0),
    };
    frame.set_cursor_position(cursor_pos);
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
