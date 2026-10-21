//! Searches Wallhaven, caches a thumbnail and downloads the smallest result into a folder:
//! `wallhaven [text] [folder]`. Doesn't change the wallpaper.

use dmm_sys::wallhaven::{Client, Query, ThumbSize};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let client = Client::new(None);
    let query = Query { text: args.next().unwrap_or_default(), ..Default::default() };
    let page = client.search(&query).await?;
    println!("page {}/{} of {} results", page.page, page.last_page, page.total);
    let first = &page.wallpapers[0];
    println!("thumbnail: {}", client.thumbnail(first, ThumbSize::Small).await?.display());
    let details = client.details(&first.id).await?;
    println!("tags: {:?}", details.tags.iter().take(5).map(|t| &t.name).collect::<Vec<_>>());
    if let Some(folder) = args.next() {
        let smallest = page.wallpapers.iter().min_by_key(|w| w.file_size).unwrap();
        let last = std::cell::Cell::new(0);
        let path = client
            .download(smallest, folder.as_ref(), |done, total| {
                let p = done * 100 / total.max(1);
                if p / 25 != last.get() / 25 {
                    println!("  {p}%");
                }
                last.set(p);
            })
            .await?;
        println!(
            "downloaded {} ({} bytes, expected {})",
            path.display(),
            std::fs::metadata(&path).unwrap().len(),
            smallest.file_size
        );
    }
    Ok(())
}
