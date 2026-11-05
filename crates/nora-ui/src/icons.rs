// //// Neoffice — added file (no upstream equivalent): the few Lucide icons of the pane, drawn from their SVG paths.

//! The pane's icons. The design system gives an application surface one icon set, **Lucide** (monochrome, one weight,
//! `currentColor`; ISC licence, see `ATTRIBUTION.md`), and none is to be redrawn: each icon here is the SVG path data of
//! the Lucide file (lucide-static 0.545), read at draw time into lines and drawn with the pane's own colour. Only the
//! path commands those files use are read (`M L H V C A Z`, absolute or relative).

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

/// One drawing primitive of an icon, in Lucide's 24 by 24 grid.
enum Part {
    Path(&'static str),
    Rect { x: f32, y: f32, w: f32, h: f32, r: f32 },
}

/// An icon of the pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Mic,
    ArrowUp,
    Close,
    Stop,
    Undo,
    NewChat,
    Check,
}

impl Icon {
    fn parts(self) -> &'static [Part] {
        match self {
            Icon::Mic => &[Part::Path("M12 19v3"), Part::Path("M19 10v2a7 7 0 0 1-14 0v-2"), Part::Rect { x: 9.0, y: 2.0, w: 6.0, h: 13.0, r: 3.0 }],
            Icon::ArrowUp => &[Part::Path("m5 12 7-7 7 7"), Part::Path("M12 19V5")],
            Icon::Close => &[Part::Path("M18 6 6 18"), Part::Path("m6 6 12 12")],
            // Lucide's `square`, filled by the caller: the stop of a request.
            Icon::Stop => &[Part::Rect { x: 4.0, y: 4.0, w: 16.0, h: 16.0, r: 3.0 }],
            Icon::Undo => &[Part::Path("M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"), Part::Path("M3 3v5h5")],
            // Lucide's `square-pen`: a new conversation is a blank page to write on.
            Icon::NewChat => &[
                Part::Path("M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"),
                Part::Path("M18.375 2.625a1 1 0 0 1 3 3l-9.013 9.014a2 2 0 0 1-.853.505l-2.873.84a.5.5 0 0 1-.62-.62l.84-2.873a2 2 0 0 1 .506-.852z"),
            ],
            Icon::Check => &[Part::Path("M20 6 9 17l-5-5")],
        }
    }
}

/// Draws `icon` in `rect` (its side is the icon's size). `filled` fills the shapes instead of stroking them (the stop).
pub fn paint(painter: &Painter, icon: Icon, rect: Rect, color: Color32, filled: bool) {
    let side = rect.width().min(rect.height());
    if !side.is_finite() || side <= 0.0 {
        return;
    }
    let unit = side / 24.0;
    let origin = rect.center() - vec2(side, side) / 2.0;
    // Lucide draws at 2 units on 24; a little lighter reads better at the pane's 16 to 20 points.
    let width = (1.75 * unit).max(1.0);
    let stroke = Stroke::new(width, color);
    let place = |p: Pos2| origin + vec2(p.x, p.y) * unit;
    for part in icon.parts() {
        match part {
            Part::Rect { x, y, w, h, r } => {
                let outline = rounded_rect(*x, *y, *w, *h, *r);
                let points: Vec<Pos2> = outline.into_iter().map(place).collect();
                if filled {
                    painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
                } else {
                    painter.add(Shape::closed_line(points, stroke));
                }
            }
            Part::Path(data) => {
                for (points, closed) in flatten(data) {
                    let points: Vec<Pos2> = points.into_iter().map(place).collect();
                    if points.len() < 2 {
                        continue;
                    }
                    if closed {
                        painter.add(Shape::closed_line(points, stroke));
                    } else {
                        // Round caps, as Lucide's: a dot at each end of an open line.
                        let (first, last) = (points[0], points[points.len() - 1]);
                        painter.add(Shape::line(points, stroke));
                        painter.circle_filled(first, width / 2.0, color);
                        painter.circle_filled(last, width / 2.0, color);
                    }
                }
            }
        }
    }
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<Pos2> {
    let mut points = Vec::new();
    let corner = |points: &mut Vec<Pos2>, cx: f32, cy: f32, from: f32| {
        for step in 0..=6 {
            let angle = (from + 90.0 * step as f32 / 6.0).to_radians();
            points.push(pos2(cx + r * angle.cos(), cy + r * angle.sin()));
        }
    };
    corner(&mut points, x + w - r, y + r, -90.0);
    corner(&mut points, x + w - r, y + h - r, 0.0);
    corner(&mut points, x + r, y + h - r, 90.0);
    corner(&mut points, x + r, y + r, 180.0);
    points
}

/// The lines of an SVG path: each subpath as its points, and whether it is closed.
fn flatten(data: &str) -> Vec<(Vec<Pos2>, bool)> {
    let mut reader = Reader { chars: data.chars().collect(), at: 0 };
    let mut out: Vec<(Vec<Pos2>, bool)> = Vec::new();
    let mut points: Vec<Pos2> = Vec::new();
    let (mut current, mut start) = (pos2(0.0, 0.0), pos2(0.0, 0.0));
    let mut command = ' ';
    loop {
        reader.skip_separators();
        let Some(next) = reader.peek() else { break };
        if next.is_ascii_alphabetic() {
            command = next;
            reader.at += 1;
            if command == 'Z' || command == 'z' {
                if !points.is_empty() {
                    out.push((std::mem::take(&mut points), true));
                }
                current = start;
                continue;
            }
        } else if command == ' ' {
            break;
        }
        let relative = command.is_ascii_lowercase();
        let base = if relative { current } else { pos2(0.0, 0.0) };
        match command.to_ascii_uppercase() {
            'M' => {
                let Some((x, y)) = reader.pair() else { break };
                if !points.is_empty() {
                    out.push((std::mem::take(&mut points), false));
                }
                current = pos2(base.x + x, base.y + y);
                start = current;
                points.push(current);
                // What follows a moveto without a command is a lineto.
                command = if relative { 'l' } else { 'L' };
            }
            'L' => {
                let Some((x, y)) = reader.pair() else { break };
                current = pos2(base.x + x, base.y + y);
                points.push(current);
            }
            'H' => {
                let Some(x) = reader.number() else { break };
                current = pos2(base.x + x, current.y);
                points.push(current);
            }
            'V' => {
                let Some(y) = reader.number() else { break };
                current = pos2(current.x, base.y + y);
                points.push(current);
            }
            'C' => {
                let (Some((x1, y1)), Some((x2, y2)), Some((x, y))) = (reader.pair(), reader.pair(), reader.pair()) else { break };
                let (p1, p2, p3) = (pos2(base.x + x1, base.y + y1), pos2(base.x + x2, base.y + y2), pos2(base.x + x, base.y + y));
                for step in 1..=8 {
                    let t = step as f32 / 8.0;
                    let u = 1.0 - t;
                    let mix = |a: f32, b: f32, c: f32, d: f32| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
                    points.push(pos2(mix(current.x, p1.x, p2.x, p3.x), mix(current.y, p1.y, p2.y, p3.y)));
                }
                current = p3;
            }
            'A' => {
                let (Some(rx), Some(ry), Some(rotation), Some(large), Some(sweep), Some((x, y))) =
                    (reader.number(), reader.number(), reader.number(), reader.flag(), reader.flag(), reader.pair())
                else {
                    break;
                };
                let end = pos2(base.x + x, base.y + y);
                arc(&mut points, current, end, rx, ry, rotation, large, sweep);
                current = end;
            }
            _ => break,
        }
    }
    if !points.is_empty() {
        out.push((points, false));
    }
    out
}

/// An elliptical arc from `from` to `to` as points (SVG's endpoint form, F.6.5 of the SVG specification).
#[allow(clippy::too_many_arguments)]
fn arc(points: &mut Vec<Pos2>, from: Pos2, to: Pos2, rx: f32, ry: f32, rotation: f32, large: bool, sweep: bool) {
    if from == to {
        return;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        points.push(to);
        return;
    }
    let (sin_p, cos_p) = rotation.to_radians().sin_cos();
    let (dx, dy) = ((from.x - to.x) / 2.0, (from.y - to.y) / 2.0);
    let (x1, y1) = (cos_p * dx + sin_p * dy, -sin_p * dx + cos_p * dy);
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }
    let numerator = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let sign = if large == sweep { -1.0 } else { 1.0 };
    let coefficient = sign * (numerator / denominator).max(0.0).sqrt();
    let (cx1, cy1) = (coefficient * rx * y1 / ry, -coefficient * ry * x1 / rx);
    let (cx, cy) = (cos_p * cx1 - sin_p * cy1 + (from.x + to.x) / 2.0, sin_p * cx1 + cos_p * cy1 + (from.y + to.y) / 2.0);
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let dot = ux * vx + uy * vy;
        let length = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
        let mut a = (dot / length).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };
    let theta = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut delta = angle((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }
    let steps = ((delta.abs() / std::f32::consts::FRAC_PI_2) * 6.0).ceil().max(2.0) as usize;
    for step in 1..=steps {
        let a = theta + delta * step as f32 / steps as f32;
        let (x, y) = (rx * a.cos(), ry * a.sin());
        points.push(pos2(cos_p * x - sin_p * y + cx, sin_p * x + cos_p * y + cy));
    }
}

/// Reads the numbers of a path's data.
struct Reader {
    chars: Vec<char>,
    at: usize,
}

impl Reader {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn skip_separators(&mut self) {
        while self.peek().is_some_and(|c| c.is_whitespace() || c == ',') {
            self.at += 1;
        }
    }

    fn number(&mut self) -> Option<f32> {
        self.skip_separators();
        let start = self.at;
        if matches!(self.peek(), Some('-' | '+')) {
            self.at += 1;
        }
        let mut dot = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.at += 1;
            } else if c == '.' && !dot {
                dot = true;
                self.at += 1;
            } else {
                break;
            }
        }
        self.chars[start..self.at].iter().collect::<String>().parse().ok()
    }

    fn pair(&mut self) -> Option<(f32, f32)> {
        let x = self.number()?;
        let y = self.number()?;
        Some((x, y))
    }

    /// An arc's flag: one digit, which may be written right against the next number.
    fn flag(&mut self) -> Option<bool> {
        self.skip_separators();
        let flag = match self.peek()? {
            '0' => false,
            '1' => true,
            _ => return None,
        };
        self.at += 1;
        Some(flag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_path_is_read_with_relative_and_implicit_commands() {
        // Lucide's `arrow-up`: a moveto followed by implicit relative linetos.
        let lines = flatten("m5 12 7-7 7 7");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, vec![pos2(5.0, 12.0), pos2(12.0, 5.0), pos2(19.0, 12.0)]);
        let lines = flatten("M12 19V5");
        assert_eq!(lines[0].0, vec![pos2(12.0, 19.0), pos2(12.0, 5.0)]);
    }

    #[test]
    fn an_arc_ends_where_the_path_says() {
        // The half circle under the microphone: from (19, 10) down, round to (5, 12), up.
        let lines = flatten("M19 10v2a7 7 0 0 1-14 0v-2");
        let points = &lines[0].0;
        let last = points[points.len() - 1];
        assert!((last.x - 5.0).abs() < 0.01 && (last.y - 10.0).abs() < 0.01, "{last:?}");
        // It goes below the line y = 12, down to y = 19, as the cup of the microphone does.
        let lowest = points.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        assert!((lowest - 19.0).abs() < 0.2, "{lowest}");
    }

    #[test]
    fn a_closed_path_is_closed_and_every_icon_reads_to_the_end() {
        let lines = flatten("M15 2H6a2 2 0 0 0-2 2v16z");
        assert!(lines[0].1);
        for icon in [Icon::Mic, Icon::ArrowUp, Icon::Close, Icon::Stop, Icon::Undo, Icon::NewChat, Icon::Check] {
            for part in icon.parts() {
                if let Part::Path(data) = part {
                    let drawn = flatten(data);
                    assert!(!drawn.is_empty() && drawn.iter().all(|(points, _)| points.len() >= 2), "{icon:?}: {data}");
                    let inside = drawn.iter().flat_map(|(points, _)| points).all(|p| (-0.5..=24.5).contains(&p.x) && (-0.5..=24.5).contains(&p.y));
                    assert!(inside, "{icon:?} stays on its 24 by 24 grid: {data}");
                }
            }
        }
    }

    #[test]
    fn an_icon_is_drawn_in_any_rect_and_never_in_none() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui.painter(), Icon::Mic, Rect::from_min_size(pos2(0.0, 0.0), vec2(16.0, 16.0)), Color32::BLACK, false);
            paint(ui.painter(), Icon::Stop, Rect::from_min_size(pos2(0.0, 0.0), vec2(16.0, 16.0)), Color32::BLACK, true);
            paint(ui.painter(), Icon::Mic, Rect::NOTHING, Color32::BLACK, false);
        });
        output.textures_delta.clear();
    }
}
