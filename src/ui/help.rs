use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn render_help_popup(frame: &mut Frame, area: Rect) {
    // Clear the area behind the popup
    frame.render_widget(Clear, area);

    let help_text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  ^/k  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("Move selection up"),
        ]),
        Line::from(vec![
            Span::styled(
                "  v/j  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("Move selection down"),
        ]),
        Line::from(vec![
            Span::styled(
                "  Enter  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("Launch Claude Code with selected profile"),
        ]),
        Line::from(vec![
            Span::styled(
                "  ?  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("Toggle this help"),
        ]),
        Line::from(vec![
            Span::styled(
                "  e  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("Edit selected profile"),
        ]),
        Line::from(vec![
            Span::styled(
                "  n  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("Create new profile"),
        ]),
        Line::from(vec![
            Span::styled(
                "  d  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("Delete selected profile"),
        ]),
        Line::from(vec![
            Span::styled(
                "  r  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("Reset selected profile to defaults"),
        ]),
        Line::from(vec![
            Span::styled(
                "  R  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("Reset ALL profiles to defaults"),
        ]),
        Line::from(vec![
            Span::styled(
                "  l  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("Select local LMStudio model"),
        ]),
        Line::from(vec![
            Span::styled(
                "  q/Esc  ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("Quit"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .style(Style::default().bg(Color::Black)),
        )
        .style(Style::default().bg(Color::Black));

    frame.render_widget(help, area);
}
