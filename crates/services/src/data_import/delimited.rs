use super::*;
mod quotes;
use std::io::{self, BufRead, Read};
pub(super) struct Decoded {
    input: BufReader<File>,
    decoder: encoding_rs::Decoder,
    output: Vec<u8>,
    position: usize,
    end: usize,
    finished: bool,
}
impl Decoded {
    pub(super) fn open(source: &Source, path: &Path) -> Result<Self> {
        let mut input = BufReader::with_capacity(64 * 1024, File::open(path)?);
        let head = input.fill_buf()?;
        let (encoding, bom) = encoding_rs::Encoding::for_bom(head).map_or_else(
            || {
                (
                    if source.encoding == "自动" {
                        Some(encoding_rs::UTF_8)
                    } else {
                        encoding_rs::Encoding::for_label(source.encoding.as_bytes())
                    },
                    0,
                )
            },
            |(e, n)| (Some(e), n),
        );
        let encoding = encoding.context("不支持的字符编码")?;
        input.consume(bom);
        Ok(Self {
            input,
            decoder: encoding.new_decoder_without_bom_handling(),
            output: vec![0; 64 * 1024],
            position: 0,
            end: 0,
            finished: false,
        })
    }
}
impl Read for Decoded {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        while self.position == self.end && !self.finished {
            let input = self.input.fill_buf()?;
            let eof = input.is_empty();
            let (result, read, written) =
                self.decoder
                    .decode_to_utf8_without_replacement(input, &mut self.output, eof);
            self.input.consume(read);
            self.position = 0;
            self.end = written;
            match result {
                encoding_rs::DecoderResult::Malformed(_, _) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "源文件包含无效字符，请检查字符编码",
                    ));
                }
                encoding_rs::DecoderResult::InputEmpty if eof => self.finished = true,
                _ => {}
            }
        }
        let n = out.len().min(self.end - self.position);
        out[..n].copy_from_slice(&self.output[self.position..self.position + n]);
        self.position += n;
        Ok(n)
    }
}
pub(super) fn read(
    source: &Source,
    path: &Path,
    directory: &Path,
    output: &mut Catalog,
    control: &Control,
) -> Result<()> {
    let mut builder = csv::ReaderBuilder::new();
    builder
        .has_headers(false)
        .flexible(true)
        .delimiter(source.delimiter as u8)
        .quoting(source.quote.is_some());
    if let Some(quote) = source.quote {
        builder.quote(quote as u8);
    }
    let (input, budget) = Budget::new(Decoded::open(source, path)?);
    let (input, flags) = quotes::Quoted::new(input, source);
    let mut reader = builder.from_reader(input);
    let mut record = csv::StringRecord::new();
    let mut fields = Vec::new();
    let mut sink = None;
    let mut index = 0u64;
    loop {
        budget.set(0);
        if !reader.read_record(&mut record)? {
            break;
        }
        let quoted = flags
            .borrow_mut()
            .pop_front()
            .context("源文本限定符与记录不一致")?;
        control.check()?;
        index += 1;
        if record.iter().map(str::len).sum::<usize>() > MAX_ROW_BYTES {
            anyhow::bail!("第 {index} 行超过 16 MiB")
        }
        if index == source.header_row as u64 {
            fields = record
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    field(if n.is_empty() {
                        format!("f{}", i + 1)
                    } else {
                        n.into()
                    })
                })
                .collect();
            let names = fields
                .iter()
                .map(|f| &f.name)
                .collect::<std::collections::HashSet<_>>();
            ensure!(
                names.len() == fields.len(),
                "字段名行包含重复名称，请修改字段名行或取消表头"
            );
            continue;
        }
        if !included(source, index) {
            continue;
        }
        if source.skip_empty
            && record.iter().all(|s| {
                if source.trim {
                    s.trim().is_empty()
                } else {
                    s.is_empty()
                }
            })
            && !quoted.iter().any(|q| *q)
        {
            continue;
        }
        if sink.is_none() {
            if fields.is_empty() {
                fields = (0..record.len())
                    .map(|i| field(format!("f{}", i + 1)))
                    .collect()
            }
            sink = Some(Sink::new(
                directory,
                output,
                name(path),
                path.display().to_string(),
                std::mem::take(&mut fields),
            )?)
        }
        let sink = sink.as_mut().unwrap();
        ensure!(
            record.len() <= sink.table.fields.len(),
            "第 {index} 行字段数超过字段名行，请检查分隔符或表头"
        );
        sink.push(
            record
                .iter()
                .enumerate()
                .map(|(i, s)| text_cell(source, s.into(), quoted.get(i) == Some(&true)))
                .collect(),
            control,
        )?;
    }
    if let Some(sink) = sink {
        sink.finish(output)
    } else {
        ensure!(!fields.is_empty(), "源文件没有记录或字段");
        Sink::new(
            directory,
            output,
            name(path),
            path.display().to_string(),
            fields,
        )?
        .finish(output)
    }
}
