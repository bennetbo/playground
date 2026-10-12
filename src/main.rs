use std::{
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal::{
        self, BeginSynchronizedUpdate, DisableLineWrap, EnableLineWrap, EndSynchronizedUpdate,
        EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use rand::Rng;
use terminal_colorsaurus::{QueryOptions, ThemeMode, theme_mode};

const TITLE: &str = "Delta at NYC AI Engineer";
const FRAME_TIME: Duration = Duration::from_millis(250);
const GLYPHS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ@#$%&*+=<>:;";

struct Terminal;

impl Terminal {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(io::stdout(), EnterAlternateScreen, DisableLineWrap, Hide)?;
        Ok(guard)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            EndSynchronizedUpdate,
            SetAttribute(Attribute::Reset),
            ResetColor,
            Show,
            EnableLineWrap,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

struct Raindrop {
    head: i32,
    length: i32,
    speed: i32,
}

impl Raindrop {
    fn new(height: u16, rng: &mut impl Rng) -> Self {
        Self {
            head: -rng.random_range(0..i32::from(height).max(1)),
            length: rng.random_range(6..25),
            speed: rng.random_range(1..4),
        }
    }
}

// ASCII keeps each rain glyph and overlay character exactly one terminal cell wide.
fn overlay_lines(username: &str, width: u16) -> [String; 2] {
    [TITLE, username].map(|line| {
        line.chars()
            .map(|ch| {
                if ch.is_ascii() && !ch.is_control() {
                    ch
                } else {
                    '?'
                }
            })
            .take(usize::from(width))
            .collect()
    })
}

fn draw_overlay(
    out: &mut impl Write,
    width: u16,
    height: u16,
    lines: &[String; 2],
    light: bool,
) -> io::Result<()> {
    if width == 0 || height == 0 {
        return Ok(());
    }
    let top = height.saturating_sub(2) / 2;
    for (offset, line) in lines.iter().take(usize::from(height)).enumerate() {
        let x = (width - line.len() as u16) / 2;
        let left = x.saturating_sub(2);
        let right = (x + line.len() as u16).saturating_add(2).min(width);
        queue!(
            out,
            MoveTo(left, top + offset as u16),
            Print(" ".repeat(usize::from(right - left))),
            MoveTo(x, top + offset as u16),
            SetForegroundColor(match (offset, light) {
                (0, false) => Color::Cyan,
                (0, true) => Color::DarkCyan,
                (_, false) => Color::Magenta,
                (_, true) => Color::DarkMagenta,
            }),
            SetAttribute(if offset == 0 {
                Attribute::Bold
            } else {
                Attribute::NormalIntensity
            }),
            Print(line),
            SetAttribute(Attribute::NormalIntensity),
        )?;
    }
    Ok(())
}

fn fill_background(out: &mut impl Write, width: u16, height: u16) -> io::Result<()> {
    // Erase commands may use the terminal's default background rather than our
    // selected color. Paint actual spaces, including the bottom-right cell.
    // Line wrapping is disabled for the lifetime of the alternate screen.
    if width > 0 {
        let row = " ".repeat(usize::from(width));
        for y in 0..height {
            queue!(out, MoveTo(0, y), Print(&row))?;
        }
    }
    Ok(())
}

fn run() -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "Run nycmatrix in an interactive terminal.",
        ));
    }

    let username = whoami::username();
    // Query before entering raw mode. Unsupported terminals fall back to dark.
    let light = matches!(theme_mode(QueryOptions::default()), Ok(ThemeMode::Light));
    let _terminal = Terminal::enter()?;
    let mut out = io::stdout().lock();
    let mut rng = rand::rng();
    let (mut width, mut height) = terminal::size()?;
    let mut drops: Vec<_> = (0..width)
        .map(|_| Raindrop::new(height, &mut rng))
        .collect();
    let mut lines = overlay_lines(&username, width);
    // Build complete frames in memory instead of writing every escape sequence
    // through stdout. Synchronized updates hide intermediate drawing states.
    let mut frame = Vec::new();

    loop {
        let deadline = Instant::now() + FRAME_TIME;
        frame.clear();
        queue!(
            frame,
            BeginSynchronizedUpdate,
            SetBackgroundColor(if light {
                Color::Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                }
            } else {
                Color::Rgb { r: 0, g: 0, b: 0 }
            }),
        )?;
        fill_background(&mut frame, width, height)?;
        for (x, drop) in drops.iter_mut().enumerate() {
            for distance in 0..drop.length {
                let y = drop.head - distance;
                if y < 0 || y >= i32::from(height) {
                    continue;
                }
                let color = if distance == 0 {
                    if light { Color::Black } else { Color::White }
                } else if distance < drop.length / 3 && !light {
                    Color::Green
                } else {
                    Color::DarkGreen
                };
                let glyph = GLYPHS[rng.random_range(0..GLYPHS.len())] as char;
                queue!(
                    frame,
                    MoveTo(x as u16, y as u16),
                    SetForegroundColor(color),
                    Print(glyph),
                )?;
            }
            drop.head += drop.speed;
            if drop.head - drop.length >= i32::from(height) {
                *drop = Raindrop::new(height, &mut rng);
            }
        }
        draw_overlay(&mut frame, width, height, &lines, light)?;
        queue!(frame, EndSynchronizedUpdate)?;
        out.write_all(&frame)?;
        out.flush()?;

        while event::poll(deadline.saturating_duration_since(Instant::now()))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        return Ok(());
                    }
                }
                Event::Resize(w, h) => {
                    width = w;
                    height = h;
                    drops = (0..width)
                        .map(|_| Raindrop::new(height, &mut rng))
                        .collect();
                    lines = overlay_lines(&username, width);
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                break;
            }
        }
    }
}

fn main() -> io::Result<()> {
    run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_paints_every_cell_without_erase_commands() {
        let mut output = Vec::new();
        fill_background(&mut output, 3, 2).unwrap();
        assert_eq!(output, b"\x1b[1;1H   \x1b[2;1H   ");

        for (width, height) in [(0, 2), (3, 0)] {
            output.clear();
            fill_background(&mut output, width, height).unwrap();
            assert!(output.is_empty());
        }
    }

    #[test]
    fn overlay_contains_title_and_username() {
        assert_eq!(overlay_lines("bennetbo", 80), [TITLE, "bennetbo"]);
    }

    #[test]
    fn overlay_uses_distinct_colors_and_only_bolds_title() {
        for light in [false, true] {
            let mut output = Vec::new();
            draw_overlay(&mut output, 80, 24, &overlay_lines("user", 80), light).unwrap();
            let output = String::from_utf8(output).unwrap();
            let title_color = if light { Color::DarkCyan } else { Color::Cyan };
            let user_color = if light {
                Color::DarkMagenta
            } else {
                Color::Magenta
            };
            assert!(output.contains(&format!(
                "{}{}{TITLE}{}",
                SetForegroundColor(title_color),
                SetAttribute(Attribute::Bold),
                SetAttribute(Attribute::NormalIntensity),
            )));
            assert!(output.contains(&format!(
                "{}{}user",
                SetForegroundColor(user_color),
                SetAttribute(Attribute::NormalIntensity),
            )));
        }
    }

    #[test]
    fn overlay_fits_small_terminals_and_sanitizes_controls() {
        assert_eq!(overlay_lines("ab\né", 4), ["Delt", "ab??"]);
        assert_eq!(overlay_lines("user", 0), ["", ""]);
        for width in 0..30 {
            for height in 0..4 {
                draw_overlay(
                    &mut Vec::new(),
                    width,
                    height,
                    &overlay_lines("user", width),
                    false,
                )
                .unwrap();
            }
        }
    }
}
