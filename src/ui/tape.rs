use crate::app::{AppState, TapeEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

pub fn render(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if state.tape.is_empty() {
        let p = Paragraph::new("No tape events")
            .block(Block::default().title("Tape").borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        f.render_widget(p, area);
        return;
    }

    // Newest first.
    let events: Vec<&TapeEvent> = state.tape.iter().rev().collect();
    let inner_h = area.height.saturating_sub(2) as usize;
    let visible = inner_h.max(1);
    let max_offset = events.len().saturating_sub(visible);
    let offset = state.scroll.min(max_offset);
    let page = &events[offset..(offset + visible).min(events.len())];

    let lines: Vec<Line> = page
        .iter()
        .map(|e| match e {
            TapeEvent::Trade {
                time,
                coin,
                side,
                px,
                sz,
            } => {
                let is_fill = state
                    .fills
                    .iter()
                    .any(|fill| fill.coin == *coin && (fill.time - time).abs() < 1000);
                let side_color = if side == "B" || side.eq_ignore_ascii_case("buy") {
                    Color::Green
                } else if side == "A" || side.eq_ignore_ascii_case("sell") {
                    Color::Red
                } else {
                    Color::Reset
                };
                let style = if is_fill {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(side_color)
                };
                Line::from(Span::styled(
                    format!("{} {} {} px={:.4} sz={:.4}", time, coin, side, px, sz),
                    style,
                ))
            }
            TapeEvent::UserEvent(v) => {
                let text = v.to_string();
                Line::from(Span::styled(
                    format!("USER {}", text),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
            }
            TapeEvent::Connected => Line::from(Span::styled(
                "CONNECTED".to_string(),
                Style::default().fg(Color::Green),
            )),
            TapeEvent::Disconnected => Line::from(Span::styled(
                "DISCONNECTED".to_string(),
                Style::default().fg(Color::Red),
            )),
        })
        .collect();

    let p = Paragraph::new(lines)
        .block(Block::default().title("Tape").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    f.render_widget(p, area);
}
