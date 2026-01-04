use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
};

use crate::app::App;

pub fn render_profile_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .config
        .profiles
        .iter()
        .map(|profile| {
            let name_line = Line::from(Span::styled(
                &profile.name,
                Style::default().add_modifier(Modifier::BOLD),
            ));
            let desc_line = Line::from(Span::styled(
                &profile.description,
                Style::default().fg(Color::Gray),
            ));
            ListItem::new(vec![name_line, desc_line, Line::from("")])
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Profiles"))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(list, area, &mut app.list_state);
}
