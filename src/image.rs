use lopdf::{Dictionary, Object};

pub fn to_base64(data: &[u8]) -> String {
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        out.push(CHARSET[(b0 >> 2) as usize] as char);
        out.push(CHARSET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARSET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARSET[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub fn extract_xobject_image(
    doc: &lopdf::Document,
    page_dict: &Dictionary,
    name_bytes: &[u8],
) -> Option<String> {
    let res_obj = page_dict.get(b"Resources").ok()?;
    let res_dict = match res_obj {
        Object::Dictionary(ref d) => d,
        Object::Reference(id) => doc.get_dictionary(*id).ok()?,
        _ => return None,
    };

    let xobj_ref = res_dict.get(b"XObject").ok()?;
    let xobj_dict = match xobj_ref {
        Object::Dictionary(ref d) => d,
        Object::Reference(id) => doc.get_dictionary(*id).ok()?,
        _ => return None,
    };

    let img_obj = xobj_dict.get(name_bytes).ok()?;
    let stream = match img_obj {
        Object::Reference(id) => doc.get_object(*id).and_then(|o| o.as_stream()).ok()?,
        Object::Stream(ref s) => s,
        _ => return None,
    };

    // JPEG detection (/Filter /DCTDecode)
    let is_jpeg = if let Ok(filter) = stream.dict.get(b"Filter") {
        match filter {
            Object::Name(ref n) => n == b"DCTDecode",
            Object::Array(ref a) => a
                .iter()
                .any(|o| o.as_name().map(|n| n == b"DCTDecode").unwrap_or(false)),
            _ => false,
        }
    } else {
        false
    };

    if is_jpeg {
        let b64 = to_base64(&stream.content);
        return Some(format!("data:image/jpeg;base64,{b64}"));
    }

    None
}
