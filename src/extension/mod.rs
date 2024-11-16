pub mod breakpoint_resume;

pub trait DownloaderExtension {
    fn on_downloaded_len_change();
    fn on_file_pause();
}
