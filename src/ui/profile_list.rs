use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

use crate::app::App;

pub fn render_profile_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let list_width = area.width.saturating_sub(4) as usize; // -2 for borders/padding, extra safety

    let items: Vec<ListItem> = app
        .config
        .profiles
        .iter()
        .map(|profile| {
            let name_line = Line::from(Span::styled(
                &profile.name,
                Style::default().add_modifier(Modifier::BOLD),
            ));

            let mut lines = vec![name_line];

            // Simple word wrapping for description
            let words: Vec<&str> = profile.description.split_whitespace().collect();
            let mut current_line = String::new();

            for word in words {
                if current_line.len() + word.len() + 1 > list_width {
                    if !current_line.is_empty() {
                        lines.push(Line::from(Span::styled(
                            current_line.clone(),
                            Style::default().fg(Color::Gray),
                        )));
                        current_line.clear();
                    }
                }
                if !current_line.is_empty() {
                    current_line.push(' ');
                }
                current_line.push_str(word);
            }
            if !current_line.is_empty() {
                lines.push(Line::from(Span::styled(
                    current_line,
                    Style::default().fg(Color::Gray),
                )));
            }

            lines.push(Line::from("")); // Spacer
            ListItem::new(lines)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::TOP).title("Profiles"))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(list, area, &mut app.list_state);
}
