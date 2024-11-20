use std::time::Instant;
use std::fmt::Write;
use std::io::stdout;
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType};
use crossterm::cursor::{MoveToPreviousLine, MoveToColumn};

pub struct ProgressBar {
    bar_buf: String,
    buf: String,
    start_instant: Instant,
    bar_width: usize
}

impl ProgressBar {
    pub fn new(max_width: usize) -> Self {
        Self {
            buf: String::new(),
            bar_buf: String::new(),
            start_instant: Instant::now(),
            bar_width: crossterm::terminal::size().ok()
                .map(|(cols, _rows)| usize::from(cols))
                .unwrap_or(0).min(max_width),
        }
    }

    fn update(&mut self, downloaded_len: u64, total_len: u64, speed: u64) -> Result<&str, std::fmt::Error> {
        let progress = (downloaded_len * 100 / total_len) as usize;

        let (downloaded_len_size, downloaded_len_unit) = Self::byte_unit(downloaded_len);
        let (total_len_size, total_len_unit) = Self::byte_unit(total_len);
        let (speed_size, speed_unit) = Self::byte_unit(speed);

        self.bar_buf.clear();
        self.buf.clear();
        let duration = self.start_instant.elapsed();
        write!(self.bar_buf, "{speed_size:.2} {speed_unit}/s - {progress} % - elapsed: {duration:.2?} ")?;
        write!(self.buf, "{downloaded_len_size:.2} {downloaded_len_unit} / {total_len_size:.2} {total_len_unit}")?;
        for _ in 0..(self.bar_width - self.bar_buf.len() - self.buf.len()) {
            self.bar_buf.push(' ');
        }
        writeln!(self.bar_buf, "{}", self.buf)?;

        let bar_p_width = self.bar_width - 2;
        let progress_width = progress * bar_p_width / 100;
        self.bar_buf.push('[');
        for _ in 0..progress_width {
            self.bar_buf.push('█');
        }
        for _ in progress_width..bar_p_width {
            self.bar_buf.push(' ');
        }
        self.bar_buf.push(']');

        Ok(&self.bar_buf)
    }

    pub fn print(&mut self, downloaded_len: u64, total_len: u64, speed: u64) {
        let update_value = self.update(downloaded_len, total_len, speed).unwrap();
        execute!(
            stdout(),
            Clear(ClearType::CurrentLine),
            MoveToPreviousLine(1),
            Clear(ClearType::CurrentLine),
            MoveToColumn(0),
            crossterm::style::Print(update_value),
        ).unwrap();
    }

    fn byte_unit(bytes_count: u64) -> (f32, &'static str) {
        const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];

        let mut i = 0;
        let mut bytes_count = bytes_count as f32;
        while bytes_count >= 1024.0 && i < UNITS.len() - 1 {
            i += 1;
            bytes_count /= 1024.0;
        }
        (bytes_count, UNITS[i])
    }
}
