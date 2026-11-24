//! Shell metadata expansion and file streams share the same cancellable export lease.
use super::*;

pub(super) fn begin(request: Request) -> Result<tomato_native_drag::Drag> {
    let preview = files(&request);
    if request.items.iter().any(|item| item.directory) {
        tomato_native_drag::begin_deferred_files(preview, move |cancel| {
            request.wait_manifest(|| cancel.cancelled())?;
            Ok(files(&request))
        })
    } else {
        tomato_native_drag::begin_files(preview)
    }
}

fn files(request: &Request) -> Vec<tomato_native_drag::File> {
    request
        .items()
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let ready = request.clone();
            let resolve = request.clone();
            let stream = request.clone();
            tomato_native_drag::File::new(
                item.relative.replace('/', "\\"),
                item.size,
                move || {
                    ready.ready(index).map(|asset| {
                        (
                            asset.path().to_owned(),
                            asset as Arc<dyn std::any::Any + Send + Sync>,
                        )
                    })
                },
                move |cancel| {
                    let asset = resolve.wait_file(index, || cancel.cancelled())?;
                    Ok((asset.path().to_owned(), asset))
                },
            )
            .with_directory(item.directory)
            .with_available(move |offset, count, cancel| {
                let asset = stream.wait_file(index, || cancel.cancelled())?;
                asset.stream.available(offset, count, || {
                    stream.cancel.is_cancelled() || cancel.cancelled()
                })
            })
        })
        .collect()
}
