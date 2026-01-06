mod help;
mod profile_list;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::borrow::Cow;

use crate::app::{
    App, AppMode, EDIT_FIELD_API_KEY, EDIT_FIELD_DESCRIPTION, EDIT_FIELD_HAIKU, EDIT_FIELD_NAME,
    EDIT_FIELD_OPUS, EDIT_FIELD_PROXY_URL, EDIT_FIELD_SONNET, EDIT_FIELD_URL,
};
use crate::config::{
    ENV_AUTH_TOKEN, ENV_BASE_URL, ENV_DEFAULT_HAIKU_MODEL, ENV_DEFAULT_OPUS_MODEL,
    ENV_DEFAULT_SONNET_MODEL, ENV_PROXY_TARGET_URL,
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
            Constraint::Min(4),               // Profile list (reduced from 8 to allow more room)
            Constraint::Length(8), // Details panel (slightly increased to ensure it fits its title/content)
            Constraint::Length(2), // Footer
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
    if let AppMode::EditProfile {
        focused_field,
        is_creating: _,
    } = app.mode
    {
        let area = centered_rect(70, 80, frame.area());
        render_edit_profile(frame, app, area, focused_field);
    }

    // Overlay model picker if in model picker mode
    if let AppMode::ModelPicker { .. } = app.mode {
        // First, render the edit form behind it
        let edit_area = centered_rect(70, 80, frame.area());
        if let AppMode::ModelPicker { target_field, .. } = app.mode {
            render_edit_profile(frame, app, edit_area, target_field);
        }
        // Then render the model picker on top
        let picker_area = centered_rect(50, 60, frame.area());
        render_model_picker(frame, app, picker_area);
    }
}

fn title_height_for_width(_w: u16, _h: u16) -> u16 {
    14 // Two-line ASCII art header
}

fn render_title(frame: &mut Frame, area: Rect) {
    let blue = Color::Rgb(90, 170, 255);
    let blue_alt = Color::Rgb(60, 140, 235);

    let art_lines = vec![
        Line::from(Span::styled(
            " ██████╗██╗     ██████╗ ██╗   ██╗██████╗ ███████╗",
            Style::default().fg(blue),
        )),
        Line::from(Span::styled(
            "██╔════╝██║     ██╔═══██╗██║   ██║██╔══██╗██╔════╝",
            Style::default().fg(blue),
        )),
        Line::from(Span::styled(
            "██║     ██║     ███████║ ██║   ██║██║  ██║█████╗  ",
            Style::default().fg(blue),
        )),
        Line::from(Span::styled(
            "██║     ██║     ██╔═══██║██║   ██║██║  ██║██╔══╝  ",
            Style::default().fg(blue),
        )),
        Line::from(Span::styled(
            "╚██████╗███████╗██║   ██║╚██████╔╝██████╔╝███████╗",
            Style::default().fg(blue),
        )),
        Line::from(Span::styled(
            " ╚═════╝╚══════╝╚═╝   ╚═╝ ╚═════╝ ╚═════╝ ╚══════╝",
            Style::default().fg(blue),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            "██████╗ ██████╗  ██████╗ ███████╗██╗██╗     ███████╗██████╗ ",
            Style::default().fg(blue_alt),
        )),
        Line::from(Span::styled(
            "██╔══██╗██╔══██╗██╔═══██╗██╔════╝██║██║     ██╔════╝██╔══██╗",
            Style::default().fg(blue_alt),
        )),
        Line::from(Span::styled(
            "██████╔╝██████╔╝██║   ██║█████╗  ██║██║     █████╗  ██████╔╝",
            Style::default().fg(blue_alt),
        )),
        Line::from(Span::styled(
            "██╔═══╝ ██╔══██╗██║   ██║██╔══╝  ██║██║     ██╔══╝  ██╔══██╗",
            Style::default().fg(blue_alt),
        )),
        Line::from(Span::styled(
            "██║     ██║  ██║╚██████╔╝██║     ██║███████╗███████╗██║  ██║",
            Style::default().fg(blue_alt),
        )),
        Line::from(Span::styled(
            "╚═╝     ╚═╝  ╚═╝ ╚═════╝ ╚═╝     ╚═╝╚══════╝╚══════╝╚═╝  ╚═╝",
            Style::default().fg(blue_alt),
        )),
    ];

    let title = Paragraph::new(art_lines)
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
            Span::styled("R", Style::default().fg(Color::Cyan)),
            Span::styled("] Reset All  ", Style::default().fg(Color::DarkGray)),
            Span::styled("[", Style::default().fg(Color::DarkGray)),
            Span::styled("q", Style::default().fg(Color::Cyan)),
            Span::styled("] Quit", Style::default().fg(Color::DarkGray)),
        ])
    };

    let footer = Paragraph::new(footer_text).block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, area);
}

fn render_edit_profile(frame: &mut Frame, app: &App, area: Rect, focused_field: usize) {
    frame.render_widget(Clear, area);

    let title = if let AppMode::EditProfile { is_creating, .. } = app.mode {
        if is_creating { " Create Profile " } else { " Edit Profile " }
    } else {
        " Edit Profile "
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

    let desc_width = inner_area.width.saturating_sub(2);
    let desc_lines = estimate_line_count(app.description_input.value(), desc_width);
    let desc_height = (desc_lines + 2).max(3);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Name
            Constraint::Length(desc_height), // Description
            Constraint::Length(3), // API Key
            Constraint::Length(3), // URL
            Constraint::Length(3), // Proxy Target URL
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
        false,
    );
    render_edit_field(
        frame,
        chunks[1],
        "Description",
        app.description_input.value(),
        focused_field == EDIT_FIELD_DESCRIPTION,
        true,
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
        false,
    );
    render_edit_field(
        frame,
        chunks[3],
        ENV_BASE_URL,
        app.url_input.value(),
        focused_field == EDIT_FIELD_URL,
        false,
    );
    render_edit_field(
        frame,
        chunks[4],
        ENV_PROXY_TARGET_URL,
        app.proxy_url_input.value(),
        focused_field == EDIT_FIELD_PROXY_URL,
        false,
    );
    render_edit_field(
        frame,
        chunks[5],
        ENV_DEFAULT_HAIKU_MODEL,
        app.haiku_model_input.value(),
        focused_field == EDIT_FIELD_HAIKU,
        false,
    );
    render_edit_field(
        frame,
        chunks[6],
        ENV_DEFAULT_SONNET_MODEL,
        app.sonnet_model_input.value(),
        focused_field == EDIT_FIELD_SONNET,
        false,
    );
    render_edit_field(
        frame,
        chunks[7],
        ENV_DEFAULT_OPUS_MODEL,
        app.opus_model_input.value(),
        focused_field == EDIT_FIELD_OPUS,
        false,
    );

    let is_model_field = matches!(
        focused_field,
        EDIT_FIELD_HAIKU | EDIT_FIELD_SONNET | EDIT_FIELD_OPUS
    );
    let show_model_picker_hint = is_model_field && app.is_codex_profile() && !app.codex_models.is_empty();

    let help_text = if show_model_picker_hint {
        Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" Switch  "),
            Span::styled("Enter", Style::default().fg(Color::Green)),
            Span::raw(" Pick Model  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" Cancel"),
        ])
    } else {
        Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" Switch  "),
            Span::styled("Ctrl+G", Style::default().fg(Color::Cyan)),
            Span::raw(" Toggle Reveal  "),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(" Save  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" Cancel"),
        ])
    };
    frame.render_widget(Paragraph::new(help_text), chunks[9]);

    // Calculate wrapped cursor for description if focused
    let (desc_cursor_x, desc_cursor_y) = if focused_field == EDIT_FIELD_DESCRIPTION {
        calculate_wrapped_cursor(
            app.description_input.value(),
            app.description_input.visual_cursor(),
            chunks[1].width.saturating_sub(2),
        )
    } else {
        (app.description_input.visual_cursor() as u16, 0)
    };

    // Set cursor
    let cursor_positions = [
        (chunks[0], app.name_input.visual_cursor() as u16, 0),
        (chunks[1], desc_cursor_x, desc_cursor_y),
        (chunks[2], app.api_key_input.visual_cursor() as u16, 0),
        (chunks[3], app.url_input.visual_cursor() as u16, 0),
        (chunks[4], app.proxy_url_input.visual_cursor() as u16, 0),
        (chunks[5], app.haiku_model_input.visual_cursor() as u16, 0),
        (chunks[6], app.sonnet_model_input.visual_cursor() as u16, 0),
        (chunks[7], app.opus_model_input.visual_cursor() as u16, 0),
    ];
    if let Some((chunk, cursor_x, cursor_y)) = cursor_positions.get(focused_field) {
        frame.set_cursor_position((chunk.x + *cursor_x + 1, chunk.y + 1 + *cursor_y));
    }
}

/// Helper function to create a centered rectangle
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = centered_layout(r, Direction::Vertical, percent_y);

    centered_layout(popup_layout[1], Direction::Horizontal, percent_x)[1]
}

fn centered_layout(area: Rect, direction: Direction, percent: u16) -> Vec<Rect> {
    Layout::default()
        .direction(direction)
        .constraints([
            Constraint::Percentage((100 - percent) / 2),
            Constraint::Percentage(percent),
            Constraint::Percentage((100 - percent) / 2),
        ])
        .split(area)
        .to_vec()
}

fn field_border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn render_edit_field(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    value: &str,
    focused: bool,
    multiline: bool,
) {
    let title_line = Line::from(vec![Span::raw(" "), Span::raw(title), Span::raw(" ")]);
    let mut paragraph = Paragraph::new(value).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title_line)
            .border_style(field_border_style(focused)),
    );

    if multiline {
        paragraph = paragraph.wrap(Wrap { trim: true });
    }

    frame.render_widget(paragraph, area);
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

fn render_model_picker(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Select Model ")
        .style(Style::default().bg(Color::Black));
    frame.render_widget(block, area);

    let inner_area = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3), // Model list
            Constraint::Length(1), // Help text
        ])
        .split(inner_area);

    // Render model list
    let models: Vec<Line> = app
        .codex_models
        .iter()
        .enumerate()
        .map(|(i, model)| {
            let is_selected = i == app.model_picker_index;
            let prefix = if is_selected { "▸ " } else { "  " };
            let style = if is_selected {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            Line::from(Span::styled(format!("{}{}", prefix, model), style))
        })
        .collect();

    let list = Paragraph::new(models).block(Block::default());
    frame.render_widget(list, chunks[0]);

    // Help text
    let help_text = Line::from(vec![
        Span::styled("↑/↓", Style::default().fg(Color::Cyan)),
        Span::raw(" Navigate  "),
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::raw(" Select  "),
        Span::styled("Esc", Style::default().fg(Color::Cyan)),
        Span::raw(" Cancel"),
    ]);
    frame.render_widget(Paragraph::new(help_text), chunks[1]);
}

/// Calculate the cursor position (col, row) for wrapped text.
/// matches the simple wrapping logic of ratatui (split by whitespace)
fn calculate_wrapped_cursor(text: &str, cursor_idx: usize, max_width: u16) -> (u16, u16) {
    if max_width == 0 {
        return (0, 0);
    }
    let max_width = max_width as usize;
    let chars: Vec<char> = text.chars().collect();
    
    // Check if cursor is at the very beginning
    if cursor_idx == 0 || chars.is_empty() {
        return (0, 0);
    }

    let mut col = 0;
    let mut current_row = 0;
    
    let mut i = 0;
    while i < chars.len() {
        // Find next word end
        let word_start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let word_end = i;
        let word_len = word_end - word_start;
        
        // Find trailing spaces
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let space_end = i;
        
        let chunk_len = space_end - word_start;
        
        // Does word fit?
        // If we are at start of line, it fits (forced).
        // If not start, checks if `col + chunk_len <= max_width`.
        // Actually Ratatui puts word on next line if it doesn't fit.
        
        if col > 0 && col + word_len > max_width {
            current_row += 1;
            col = 0;
        }
        
        // Check if cursor is in this chunk (word + spaces)
        if cursor_idx >= word_start && cursor_idx <= space_end {
            let offset_in_chunk = cursor_idx - word_start;
            return ((col + offset_in_chunk) as u16, current_row);
        }
        
        col += chunk_len;
    }
    
    // If cursor is at the very end of text
    if cursor_idx == chars.len() {
        return (col as u16, current_row);
    }
    
    (0, 0)
}

fn estimate_line_count(text: &str, max_width: u16) -> u16 {
    if max_width == 0 {
        return 1;
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return 1;
    }

    let mut lines = 1;
    let mut col = 0;
    
    let mut i = 0;
    while i < chars.len() {
        // Find next word end
        let word_start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let word_end = i;
        let word_len = word_end - word_start;
        
        // Find trailing spaces
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let space_end = i;
        let chunk_len = space_end - word_start;
        
        if col > 0 && col + word_len > max_width as usize {
            lines += 1;
            col = 0;
        }
        
        col += chunk_len;
    }
    
    lines
}


