use std::cmp::Reverse;

use chrono::{DateTime, Local, NaiveDate, TimeDelta, Timelike, Utc};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Layout, Margin, Rect, Spacing},
    style::{Color, Modifier, Style},
    symbols::{self, merge::MergeStrategy},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Widget, Wrap},
};

use crate::{
    app::{ActiveField, App, AppMode, EventNode, InsertEvent},
    constants::{GOOGLE_EVENT_COLORS, RESOLUTION_IN_MINS},
};

#[derive(Debug)]
struct RenderedEvent<'a> {
    pub event: &'a EventNode,
    pub rect: Rect,
}

struct VisibleEvents<'a> {
    pub event: &'a EventNode,
    pub start_row: u16,
    pub end_row: u16,
}

fn calculate_viewport_rect<'a, 'b>(
    events: &'a [EventNode],
    viewport_start: DateTime<Utc>,
    viewport_end: DateTime<Utc>,
    column_area: &'b Rect,
) -> Vec<RenderedEvent<'a>> {
    // 1. Filter and convert time directly to grid rows
    let mut visible_events: Vec<VisibleEvents<'a>> = events
        .iter()
        .filter_map(|ev| {
            if ev.is_all_day {
                return None;
            }
            if ev.end_time <= viewport_start || ev.start_time >= viewport_end {
                return None;
            }

            let clamped_start = ev.start_time.max(viewport_start);
            let clamped_end = ev.end_time.min(viewport_end);

            // Calculate total minutes from top of the screen
            let start_mins = (clamped_start - viewport_start).num_minutes();
            let end_mins = (clamped_end - viewport_start).num_minutes();

            // Convert minutes to terminal rows
            let start_row = start_mins as u16 / RESOLUTION_IN_MINS;
            let mut end_row = end_mins as u16 / RESOLUTION_IN_MINS;

            // INFO: Ensure even short events (eg. 5 mins) take up at least 1 row
            if end_row <= start_row {
                end_row = start_row + 1;
            }

            Some(VisibleEvents {
                event: ev,
                start_row,
                end_row,
            })
        })
        .collect();

    if visible_events.is_empty() {
        return vec![];
    }

    // 2. Sort and Cluster (Using Row Indices)
    visible_events.sort_by_key(|e| (e.start_row, Reverse(e.end_row)));

    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut current_cluster: Vec<usize> = Vec::new();
    let mut cluster_end_row = 0;

    for (i, ev) in visible_events.iter().enumerate() {
        if ev.start_row >= cluster_end_row && !current_cluster.is_empty() {
            clusters.push(std::mem::take(&mut current_cluster));
            cluster_end_row = ev.end_row;
        } else {
            cluster_end_row = cluster_end_row.max(ev.end_row);
        }
        current_cluster.push(i);
    }

    if !current_cluster.is_empty() {
        clusters.push(current_cluster);
    }

    // 3. Generate Rects and find the Column index
    let mut results = Vec::with_capacity(visible_events.len());

    for cluster in clusters {
        let mut column_ends: Vec<u16> = Vec::new();
        let mut placements = Vec::new();

        for &idx in &cluster {
            let ev = &visible_events[idx];
            let mut placed_col = None;

            for (c_idx, c_end) in column_ends.iter_mut().enumerate() {
                if *c_end <= ev.start_row {
                    *c_end = ev.end_row;
                    placed_col = Some(c_idx);
                    break;
                }
            }

            let col_idx = placed_col.unwrap_or_else(|| {
                column_ends.push(ev.end_row);
                column_ends.len() - 1
            });
            placements.push((idx, col_idx));
        }

        let total_cols = column_ends.len() as u16;
        let col_width = (column_area.width / total_cols).max(1);

        for (idx, col_idx) in placements {
            let ev = &visible_events[idx];

            let rect = Rect {
                x: column_area.x + (col_idx as u16 * col_width),
                y: column_area.y + ev.start_row,
                width: col_width,
                height: ev.end_row - ev.start_row,
            };

            results.push(RenderedEvent {
                event: ev.event,
                rect,
            });
        }
    }

    results
}

fn insert_event_to_rect(
    insert_event: &InsertEvent,
    viewport_start: DateTime<Utc>,
    viewport_end: DateTime<Utc>,
    column_area: &Rect,
) -> Rect {
    let end_time = insert_event.start_time + insert_event.duration;

    let clamped_start = insert_event.start_time.max(viewport_start);
    let clamped_end = end_time.min(viewport_end);

    // Calculate total minutes from top of the screen
    let start_mins = (clamped_start - viewport_start).num_minutes();
    let end_mins = (clamped_end - viewport_start).num_minutes();

    // Convert minutes to terminal rows
    let start_row = start_mins as u16 / RESOLUTION_IN_MINS;
    let end_row = end_mins as u16 / RESOLUTION_IN_MINS;

    Rect {
        x: column_area.x,
        y: column_area.y + start_row,
        width: column_area.width,
        height: end_row.saturating_sub(start_row).max(2),
    }
}

pub fn get_contrasting_text_color(color: Color) -> Color {
    match color {
        Color::Rgb(r, g, b) => {
            // ITU-R BT.601 perceived luminance
            let lum = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
            if lum > 140 {
                Color::Black
            } else {
                Color::White
            }
        }
        Color::Yellow
        | Color::LightYellow
        | Color::Cyan
        | Color::LightCyan
        | Color::White
        | Color::LightGreen => Color::Black,
        _ => Color::White,
    }
}

pub fn format_12h_parts(hour: u32, min: u32) -> (u32, u32, &'static str) {
    let (h12, ampm) = match hour {
        0 => (12, "AM"),
        1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        _ => (hour - 12, "PM"),
    };
    (h12, min, ampm)
}

pub fn format_time_str(hour: u32, min: u32, use_24h: bool) -> String {
    if use_24h {
        format!("{:02}:{:02}", hour, min)
    } else {
        let (h12, m, ampm) = format_12h_parts(hour, min);
        format!("{}:{:02} {}", h12, m, ampm)
    }
}

pub fn format_grid_line(hour: u16, width: usize, use_24h: bool) -> String {
    let prefix = if use_24h {
        format!("─ {:02}:00 ", hour)
    } else {
        let (h12, _, ampm) = format_12h_parts(hour as u32, 0);
        format!("─ {}:00 {} ", h12, ampm)
    };
    let prefix_len = prefix.chars().count();
    if width >= prefix_len + 2 {
        let remaining = width.saturating_sub(prefix_len);
        format!("{}{}", prefix, "─".repeat(remaining))
    } else {
        "─".repeat(width)
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Blocks
        let block = Block::bordered().merge_borders(MergeStrategy::Exact);

        // headers
        let target_dates: Vec<NaiveDate> = (0..self.num_days.num_days())
            .map(|i| self.start_date + TimeDelta::days(i))
            .collect();

        // Horizontal layout with dynamic constraints (1 Time column + 1 for each day)
        let gutter_width = if self.use_24h_clock { 9 } else { 13 };
        let mut h_constraints = vec![Constraint::Length(gutter_width)];
        for _ in 0..self.num_days.num_days() {
            h_constraints.push(Constraint::Fill(1));
        }
        let horizontal_layout = Layout::horizontal(h_constraints).spacing(Spacing::Overlap(1));

        // Check for all-day events across the currently visible dates
        let has_all_day = self
            .cal_event_nodes
            .iter()
            .any(|e| e.is_all_day && target_dates.iter().any(|d| e.spans_date(*d)));
        let all_day_height = if has_all_day { 2 } else { 0 };

        let v_constraints = if all_day_height > 0 {
            vec![
                Constraint::Length(3),
                Constraint::Length(all_day_height),
                Constraint::Fill(1),
                Constraint::Length(1),
            ]
        } else {
            vec![
                Constraint::Length(3),
                Constraint::Fill(1),
                Constraint::Length(1),
            ]
        };

        let vertical_areas = Layout::vertical(v_constraints)
            .spacing(Spacing::Overlap(1))
            .split(area);

        let (header, all_day_area, calendar, status_area) = if all_day_height > 0 {
            (
                vertical_areas[0],
                Some(vertical_areas[1]),
                vertical_areas[2],
                vertical_areas[3],
            )
        } else {
            (vertical_areas[0], None, vertical_areas[1], vertical_areas[2])
        };

        let header_columns = horizontal_layout.split(header);
        let columns = horizontal_layout.split(calendar);

        let today = self.now.date_naive();

        // Time header
        let time_title = if self.is_fetching { "Sync..." } else { "Time" };
        let time_style = if self.is_fetching {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        Paragraph::new(time_title)
            .style(time_style)
            .block(block.clone())
            .centered()
            .render(header_columns[0], buf);

        // Day headers: day of week + yyyy/mm/dd (e.g. "Sunday 2026/10/11")
        for (d, &header_column) in target_dates.iter().zip(header_columns.iter().skip(1)) {
            let title = d.format("%A %Y/%m/%d").to_string();
            let is_today = *d == today;
            let (header_block, style) = if is_today {
                (
                    block.clone().border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )
            } else {
                (block.clone(), Style::default())
            };

            Paragraph::new(title)
                .style(style)
                .block(header_block)
                .centered()
                .render(header_column, buf);
        }

        // Draw All-Day shelf if any all-day events are visible
        if let Some(shelf_area) = all_day_area {
            let all_day_columns = horizontal_layout.split(shelf_area);
            Paragraph::new("All-day")
                .style(Style::default().fg(Color::DarkGray))
                .block(block.clone())
                .centered()
                .render(all_day_columns[0], buf);

            for (d, &col_rect) in target_dates.iter().zip(all_day_columns.iter().skip(1)) {
                let day_all_day: Vec<&EventNode> = self
                    .cal_event_nodes
                    .iter()
                    .filter(|e| e.is_all_day && e.spans_date(*d))
                    .collect();
                let mut spans = Vec::new();
                for (idx, e) in day_all_day.iter().enumerate() {
                    if idx > 0 {
                        spans.push(Span::raw(" "));
                    }
                    let ev_color = e.get_color();
                    let text_color = get_contrasting_text_color(ev_color);
                    spans.push(Span::styled(
                        format!(" {} ", e.summary),
                        Style::default()
                            .bg(ev_color)
                            .fg(text_color)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                Paragraph::new(Line::from(spans))
                    .block(block.clone())
                    .render(col_rect, buf);
            }
        }

        // Time Gutter
        let start_mins = self.scroll_offset;
        let end_mins = self.scroll_offset + self.viewport_mins;

        let mut time_lines = Vec::new();
        for row_time in (start_mins..end_mins).step_by(RESOLUTION_IN_MINS as usize) {
            let hour = row_time / 60;
            match row_time % 60 {
                0 => {
                    let label = if self.use_24h_clock {
                        format!("{:02}:00", hour)
                    } else {
                        let (h12, _, ampm) = format_12h_parts(hour as u32, 0);
                        format!("{:2}:00 {}", h12, ampm)
                    };
                    time_lines.push(
                        Line::from(label).style(Style::default().fg(Color::Gray)),
                    );
                }
                30 => {
                    let label = if self.use_24h_clock {
                        format!("{:02}:30", hour)
                    } else {
                        let (h12, _, ampm) = format_12h_parts(hour as u32, 30);
                        format!("{:2}:30 {}", h12, ampm)
                    };
                    time_lines.push(
                        Line::from(label).style(Style::default().fg(Color::DarkGray)),
                    );
                }
                _ => {
                    time_lines.push(Line::from(""));
                }
            }
        }

        Paragraph::new(time_lines)
            .alignment(Alignment::Center)
            .block(block.clone())
            .render(columns[0], buf);

        // Draw current time cursor in gutter
        if self.is_now_timeline_visible {
            let current_mins = (self.now.hour() * 60 + self.now.minute()) as u16;
            if current_mins >= start_mins && current_mins < end_mins {
                let gutter_inner = columns[0].inner(Margin {
                    horizontal: 1,
                    vertical: 1,
                });
                let y_offset = (current_mins - start_mins) / RESOLUTION_IN_MINS;
                if y_offset < gutter_inner.height {
                    let cursor_rect = Rect {
                        x: gutter_inner.x,
                        y: gutter_inner.y + y_offset,
                        width: gutter_inner.width,
                        height: 1,
                    };
                    let cursor_text = if self.use_24h_clock {
                        format!("{:02}:{:02} >", self.now.hour(), self.now.minute())
                    } else {
                        let (h12, m, ampm) = format_12h_parts(self.now.hour(), self.now.minute());
                        format!("{}:{:02} {} >", h12, m, ampm)
                    };
                    Paragraph::new(cursor_text)
                        .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                        .render(cursor_rect, buf);
                }
            }
        }

        // Draw events
        for (day, day_area) in target_dates.iter().zip(columns.iter().skip(1)) {
            block.clone().render(*day_area, buf);

            let inner_area = day_area.inner(Margin {
                horizontal: 1,
                vertical: 1,
            });

            // 1. Draw light grid lines with time in the table
            for row_time in (start_mins..end_mins).step_by(RESOLUTION_IN_MINS as usize) {
                let y_offset = (row_time - start_mins) / RESOLUTION_IN_MINS;
                if y_offset >= inner_area.height {
                    break;
                }
                let line_rect = Rect {
                    x: inner_area.x,
                    y: inner_area.y + y_offset,
                    width: inner_area.width,
                    height: 1,
                };

                if row_time % 60 == 0 {
                    let hour = row_time / 60;
                    let line_str = format_grid_line(hour, inner_area.width as usize, self.use_24h_clock);
                    Paragraph::new(line_str)
                        .style(Style::default().fg(Color::Rgb(65, 70, 80)))
                        .render(line_rect, buf);
                } else if row_time % 30 == 0 {
                    let line_str = "┄".repeat(inner_area.width as usize);
                    Paragraph::new(line_str)
                        .style(Style::default().fg(Color::Rgb(45, 50, 60)))
                        .render(line_rect, buf);
                }
            }

            let midnight = day.and_hms_opt(0, 0, 0).unwrap_or_default();
            let start_of_day_local = crate::app::safe_local_datetime(midnight);

            let viewport_start =
                (start_of_day_local + TimeDelta::minutes(start_mins as i64)).with_timezone(&Utc);
            let viewport_end =
                (start_of_day_local + TimeDelta::minutes(end_mins as i64)).with_timezone(&Utc);

            let render_events = calculate_viewport_rect(
                &self.cal_event_nodes,
                viewport_start,
                viewport_end,
                &inner_area,
            );

            // 2. Render events on top of grid lines with vibrant calendar colors and contrasting fonts
            for re in render_events {
                let is_selected = self.sel_event_id.as_ref() == Some(&re.event.id);

                let ev_color = re.event.get_color();
                let text_color = get_contrasting_text_color(ev_color);

                let (border_color, bg_color, final_text_color) = if is_selected {
                    let sel_border = if text_color == Color::Black {
                        Color::Black
                    } else {
                        Color::Yellow
                    };
                    (sel_border, ev_color, text_color)
                } else {
                    (text_color, ev_color, text_color)
                };

                let local_start = re.event.start_time.with_timezone(&Local);
                let local_end = re.event.end_time.with_timezone(&Local);

                let summary_line = if is_selected {
                    format!("▶ {}", re.event.summary)
                } else {
                    format!(" {}", re.event.summary)
                };

                let time_range_str = if self.use_24h_clock {
                    format!(
                        " {:02}:{:02}-{:02}:{:02}",
                        local_start.hour(),
                        local_start.minute(),
                        local_end.hour(),
                        local_end.minute()
                    )
                } else {
                    let (sh, sm, sampm) = format_12h_parts(local_start.hour(), local_start.minute());
                    let (eh, em, eampm) = format_12h_parts(local_end.hour(), local_end.minute());
                    format!(
                        " {}:{:02} {} - {}:{:02} {}",
                        sh, sm, sampm, eh, em, eampm
                    )
                };

                let lines = if re.rect.height >= 2 {
                    vec![
                        Line::from(summary_line).style(
                            Style::default()
                                .fg(final_text_color)
                                .bg(bg_color)
                                .add_modifier(if is_selected {
                                    Modifier::BOLD | Modifier::UNDERLINED
                                } else {
                                    Modifier::BOLD
                                }),
                        ),
                        Line::from(time_range_str)
                            .style(
                                Style::default()
                                    .fg(final_text_color)
                                    .bg(bg_color),
                            ),
                    ]
                } else {
                    vec![
                        Line::from(summary_line).style(
                            Style::default()
                                .fg(final_text_color)
                                .bg(bg_color)
                                .add_modifier(if is_selected {
                                    Modifier::BOLD | Modifier::UNDERLINED
                                } else {
                                    Modifier::BOLD
                                }),
                        ),
                    ]
                };

                Paragraph::new(lines)
                    .block(
                        Block::default().borders(Borders::LEFT).border_style(
                            Style::default()
                                .fg(border_color)
                                .bg(bg_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                    )
                    .style(Style::default().bg(bg_color))
                    .wrap(Wrap { trim: true })
                    .render(re.rect, buf);
            }

            // Draw current time line
            if self.is_now_timeline_visible && *day == self.now.date_naive() {
                let current_mins = (self.now.hour() * 60 + self.now.minute()) as u16;
                let start_mins = self.scroll_offset;
                let end_mins = start_mins + self.viewport_mins;

                if current_mins >= start_mins && current_mins < end_mins {
                    let rel_mins = current_mins - start_mins;
                    let y_offset = rel_mins / RESOLUTION_IN_MINS;

                    let timeline_rect = Rect {
                        x: inner_area.x,
                        y: inner_area.y + y_offset,
                        width: inner_area.width,
                        height: 1,
                    };

                    let line_width = inner_area.width.saturating_sub(1) as usize;
                    let line_str = format!("{}", "━".repeat(line_width));

                    Paragraph::new(line_str)
                        .style(Style::default().fg(Color::Red))
                        .render(timeline_rect, buf);
                }
            }

            // Draw insert time block
            if self.mode == AppMode::Insert {
                let insert_end_time = self.insert_event.start_time + self.insert_event.duration;
                let is_overlapping =
                    self.insert_event.start_time < viewport_end && insert_end_time > viewport_start;
                if is_overlapping {
                    let insert_rect = insert_event_to_rect(
                        &self.insert_event,
                        viewport_start,
                        viewport_end,
                        &inner_area,
                    );
                    let local_start = self.insert_event.start_time.with_timezone(&Local);
                    let local_end = (self.insert_event.start_time + self.insert_event.duration).with_timezone(&Local);
                    let dur_mins = self.insert_event.duration.num_minutes();
                    let insert_title = if self.use_24h_clock {
                        format!(" {:02}:{:02} - {:02}:{:02} ({}m) ", local_start.hour(), local_start.minute(), local_end.hour(), local_end.minute(), dur_mins)
                    } else {
                        let (sh, sm, sampm) = format_12h_parts(local_start.hour(), local_start.minute());
                        let (eh, em, eampm) = format_12h_parts(local_end.hour(), local_end.minute());
                        format!(" {}:{:02} {} - {}:{:02} {} ({}m) ", sh, sm, sampm, eh, em, eampm, dur_mins)
                    };
                    let block_color = self.insert_event.get_color();
                    Block::bordered()
                        .border_type(ratatui::widgets::BorderType::Rounded)
                        .title(insert_title)
                        .border_style(Style::default().fg(block_color).add_modifier(Modifier::BOLD))
                        .render(insert_rect, buf);
                }
            }
        }

        // Draw Status Bar
        let mode_span = match self.mode {
            AppMode::Normal => Span::styled(" NORMAL ", Style::default().bg(Color::Green).fg(Color::Black).add_modifier(Modifier::BOLD)),
            AppMode::Insert => Span::styled(" INSERT ", Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD)),
            AppMode::Visual => Span::styled(" VISUAL ", Style::default().bg(Color::Magenta).fg(Color::Black).add_modifier(Modifier::BOLD)),
            AppMode::InsertEdit | AppMode::VisualEdit => Span::styled(" EDIT ", Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)),
            AppMode::DeleteConfirm => Span::styled(" DELETE ", Style::default().bg(Color::Red).fg(Color::White).add_modifier(Modifier::BOLD)),
        };

        let view_badge = match self.num_days.num_days() {
            1 => " [1-Day] ",
            7 => " [Week] ",
            _ => " [3-Day] ",
        };
        let view_span = Span::styled(view_badge, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

        let clock_badge = if self.use_24h_clock { " [24h] " } else { " [12h] " };
        let clock_span = Span::styled(clock_badge, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

        let slot_info = if self.mode == AppMode::Insert {
            let local_start = self.insert_event.start_time.with_timezone(&Local);
            let local_end = (self.insert_event.start_time + self.insert_event.duration).with_timezone(&Local);
            let dur_mins = self.insert_event.duration.num_minutes();
            let color_name = GOOGLE_EVENT_COLORS
                .iter()
                .find(|c| c.id == self.insert_event.color_id.as_deref())
                .map(|c| c.name)
                .unwrap_or("Default");
            if self.use_24h_clock {
                format!(" {:02}:{:02}-{:02}:{:02} ({}m) [{}] ", local_start.hour(), local_start.minute(), local_end.hour(), local_end.minute(), dur_mins, color_name)
            } else {
                let (sh, sm, sampm) = format_12h_parts(local_start.hour(), local_start.minute());
                let (eh, em, eampm) = format_12h_parts(local_end.hour(), local_end.minute());
                format!(" {}:{:02} {} - {}:{:02} {} ({}m) [{}] ", sh, sm, sampm, eh, em, eampm, dur_mins, color_name)
            }
        } else if let Some(ev) = self.get_selected_event() {
            format!(" \"{}\" ", ev.summary)
        } else if self.use_24h_clock {
            format!(" {:02}:{:02} ", self.now.hour(), self.now.minute())
        } else {
            let (h, m, ampm) = format_12h_parts(self.now.hour(), self.now.minute());
            format!(" {}:{:02} {} ", h, m, ampm)
        };
        let slot_span = Span::styled(slot_info, Style::default().fg(Color::White));

        let sync_span = if self.is_fetching {
            Span::styled(" ⟳ Syncing... ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(" ● Synced ", Style::default().fg(Color::Green))
        };
        let cals_span = Span::styled(format!(" {} cals ", self.cal_infos.len().max(1)), Style::default().fg(Color::DarkGray));
        let help_hint = if self.mode == AppMode::Insert {
            Span::styled(" ? Help | c Color | Enter Create | Esc Cancel ", Style::default().fg(Color::DarkGray))
        } else {
            Span::styled(" ? Help | d/3/w View | C 12/24h | i Block | v Visual | c Cal | q Quit ", Style::default().fg(Color::DarkGray))
        };

        let status_line = Line::from(vec![
            mode_span,
            view_span,
            clock_span,
            slot_span,
            Span::raw(" | "),
            sync_span,
            cals_span,
            Span::raw(" | "),
            help_hint,
        ]);
        Paragraph::new(status_line).render(status_area, buf);

        // Draw Popup
        if self.mode == AppMode::InsertEdit || self.mode == AppMode::VisualEdit {
            let popup_area = area.centered(Constraint::Length(62), Constraint::Length(21));

            Clear.render(popup_area, buf);
            let popup_title = match self.mode {
                AppMode::InsertEdit => " Create Event ",
                AppMode::VisualEdit => " Edit Event ",
                _ => unreachable!(),
            };

            Block::bordered()
                .title(popup_title)
                .border_style(Style::default().fg(Color::Cyan))
                .render(popup_area, buf);

            let popup_inner_area = popup_area.inner(Margin {
                horizontal: 1,
                vertical: 1,
            });

            let [summary_area, description_area, time_area, cal_area, color_area] =
                popup_inner_area.layout(&Layout::vertical([
                    Constraint::Length(3),
                    Constraint::Length(5),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                ]));

            let [start_time_area, end_time_area] = time_area.layout(&Layout::horizontal([
                Constraint::Fill(1),
                Constraint::Fill(1),
            ]));

            self.popup.summary.render(summary_area, buf);
            self.popup.description.render(description_area, buf);
            self.popup.start_time.render(start_time_area, buf);
            self.popup.end_time.render(end_time_area, buf);

            // Calendar selector
            let is_cal_active = self.popup.active_field == ActiveField::Calendar;
            let cal_border_style = if is_cal_active {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let cal_block = Block::bordered()
                .title(" Calendar (h/l cycle) ")
                .border_style(cal_border_style);

            let cal_idx = self.get_selected_cal_index();
            let cals: Vec<_> = self
                .cal_infos
                .iter()
                .map(|c| c.display_name.as_str())
                .collect();
            let cal_inner_area = cal_area.inner(Margin {
                horizontal: 1,
                vertical: 1,
            });
            cal_block.render(cal_area, buf);
            Tabs::new(cals)
                .style(Style::default().fg(Color::DarkGray))
                .highlight_style(Style::default().yellow().bold())
                .select(cal_idx)
                .divider(symbols::DOT)
                .padding(" ", " ")
                .render(cal_inner_area, buf);

            // Color selector
            let is_color_active = self.popup.active_field == ActiveField::Color;
            let color_border_style = if is_color_active {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let color_block = Block::bordered()
                .title(" Color ")
                .border_style(color_border_style);

            let current_color_idx = GOOGLE_EVENT_COLORS
                .iter()
                .position(|c| c.id == self.popup.selected_color_id.as_deref())
                .unwrap_or(0);
            let current_color = &GOOGLE_EVENT_COLORS[current_color_idx];

            let swatch_color = Color::Rgb(current_color.r, current_color.g, current_color.b);
            let swatch = Span::styled(" ● ", Style::default().fg(swatch_color).add_modifier(Modifier::BOLD));
            let name = Span::styled(
                format!("{:<20}", current_color.name),
                Style::default().fg(swatch_color).add_modifier(Modifier::BOLD),
            );
            let arrow_hint = if self.popup.color_dropdown_open {
                " ▲ (j/k select, Enter/Space confirm)"
            } else {
                " ▼ (Space/j to open, h/l cycle)"
            };
            let arrow_span = Span::styled(arrow_hint, Style::default().fg(Color::DarkGray));

            let color_line = Line::from(vec![swatch, name, arrow_span]);
            Paragraph::new(color_line)
                .block(color_block)
                .render(color_area, buf);

            // Floating dropdown list when open
            if self.popup.color_dropdown_open {
                let dropdown_height = (GOOGLE_EVENT_COLORS.len() as u16 + 2).min(area.height.saturating_sub(2));
                let dropdown_y = if color_area.bottom() + dropdown_height <= area.bottom() {
                    color_area.bottom()
                } else {
                    color_area.y.saturating_sub(dropdown_height)
                };
                let dropdown_area = Rect {
                    x: color_area.x,
                    y: dropdown_y,
                    width: color_area.width,
                    height: dropdown_height,
                };
                Clear.render(dropdown_area, buf);
                let dropdown_block = Block::bordered()
                    .title(" Select Event Color (j/k or ↑/↓, Enter select, Esc close) ")
                    .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

                let mut lines = Vec::new();
                for (idx, color) in GOOGLE_EVENT_COLORS.iter().enumerate() {
                    let is_sel = idx == current_color_idx;
                    let prefix = if is_sel { " ▶ " } else { "   " };
                    let c_swatch = "● ";
                    let line_text = format!("{}{}{}", prefix, c_swatch, color.name);
                    let color_rgb = Color::Rgb(color.r, color.g, color.b);
                    let text_style = if is_sel {
                        Style::default()
                            .fg(color_rgb)
                            .bg(Color::Rgb(40, 44, 52))
                            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                    } else {
                        Style::default().fg(color_rgb)
                    };
                    lines.push(Line::from(line_text).style(text_style));
                }
                Paragraph::new(lines)
                    .block(dropdown_block)
                    .render(dropdown_area, buf);
            }
        }

        if self.mode == AppMode::DeleteConfirm {
            let popup_area = area.centered(Constraint::Percentage(40), Constraint::Percentage(40));
            Clear.render(popup_area, buf);
            if let Some(event_node) = self.get_selected_event() {
                Paragraph::new(vec![
                    Line::from("Are you sure you want to delete this Event?"),
                    Line::from(event_node.summary.as_str()),
                    Line::from("(Enter) Yes    |    (Esc) Cancel"),
                ])
                .alignment(Alignment::Center)
                .block(
                    Block::bordered()
                        .title(" Confirm ")
                        .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                )
                .render(popup_area, buf);
            }
        }

        if self.show_help {
            let help_area = area.centered(Constraint::Percentage(65), Constraint::Percentage(70));
            Clear.render(help_area, buf);
            let help_text = vec![
                Line::from(vec![
                    ratatui::text::Span::styled("--- Navigation ---", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]),
                Line::from("  h / l       Scroll day backward / forward"),
                Line::from("  j / k       Scroll down / up (accelerates when held)"),
                Line::from("  J / K       Fast scroll (2 hours down / up)"),
                Line::from("  Ctrl-d / u  Half page scroll down / up"),
                Line::from("  Ctrl-f / b  Full page scroll down / up"),
                Line::from("  gg          Jump to top of day (00:00)"),
                Line::from("  G  (or GG)  Jump to bottom of day (23:55)"),
                Line::from("  rn (or t)   Jump to right now (current time)"),
                Line::from("  T           Toggle current time line"),
                Line::from("  R           Refresh calendar events"),
                Line::from("  c           Cycle selected calendar"),
                Line::from("  u           Undo last deleted event"),
                Line::from(""),
                Line::from(vec![
                    ratatui::text::Span::styled("--- 5m Time-Blocking ('i') ---", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]),
                Line::from("  j / k       Move time block by 5 mins"),
                Line::from("  J / K       Expand / shrink duration by 5 mins"),
                Line::from("  h / l       Move block by -1 / +1 day"),
                Line::from("  c           Cycle block color"),
                Line::from("  Enter       Open title prompt -> Enter to commit"),
                Line::from("  Esc         Cancel block"),
                Line::from(""),
                Line::from(vec![
                    ratatui::text::Span::styled("--- View Modes ---", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]),
                Line::from("  d           Day view (1 day)"),
                Line::from("  3 (or D)    3-Day view"),
                Line::from("  w           Week view (7 days)"),
                Line::from("  C           Toggle 12h / 24h clock format"),
                Line::from(""),
                Line::from(vec![
                    ratatui::text::Span::styled("--- Macros (Normal Mode) ---", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]),
                Line::from("  1 (or m d)  60m Deep Work block"),
                Line::from("  2 (or m s)  15m Standup block"),
                Line::from("  m l (or m 3) 45m Lunch block"),
                Line::from("  4 (or m p)  30m Planning block"),
                Line::from(""),
                Line::from(vec![
                    ratatui::text::Span::styled("--- Visual Mode ('v') ---", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]),
                Line::from("  j / k       Select next / prev event"),
                Line::from("  + / -       Shift selected event by +/- 5 mins"),
                Line::from("  J / K       Resize selected event by +/- 5 mins"),
                Line::from("  Enter       Edit event details"),
                Line::from("  d           Delete selected event"),
                Line::from("  u           Undo last deleted event"),
                Line::from(""),
                Line::from(vec![
                    ratatui::text::Span::styled("Press '?' or 'Esc' to close this help", Style::default().fg(Color::DarkGray)),
                ]),
            ];
            Paragraph::new(help_text)
                .block(
                    Block::bordered()
                        .title(" Keybindings & Cheat Sheet (?) ")
                        .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                )
                .render(help_area, buf);
        }
    }
}
