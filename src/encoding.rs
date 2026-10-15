//! 텍스트 파일의 인코딩: 알아내기(BOM · UTF-8 · 옛 인코딩 감지)와 바이트 ↔ 글자 변환.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    /// 한국어 완성형/확장 완성형 (CP949)
    EucKr,
    /// 일본어 (CP932)
    ShiftJis,
    /// 중국어 간체 (CP936)
    Gbk,
    /// 중국어 번체 (CP950)
    Big5,
    /// 서유럽 (영어 Windows 의 ANSI)
    Windows1252,
}

impl TextEncoding {
    pub const ALL: [TextEncoding; 9] = [
        TextEncoding::Utf8,
        TextEncoding::Utf8Bom,
        TextEncoding::Utf16Le,
        TextEncoding::Utf16Be,
        TextEncoding::EucKr,
        TextEncoding::ShiftJis,
        TextEncoding::Gbk,
        TextEncoding::Big5,
        TextEncoding::Windows1252,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TextEncoding::Utf8 => "UTF-8",
            TextEncoding::Utf8Bom => "UTF-8 (BOM)",
            TextEncoding::Utf16Le => "UTF-16 LE",
            TextEncoding::Utf16Be => "UTF-16 BE",
            TextEncoding::EucKr => "ANSI (CP949)",
            TextEncoding::ShiftJis => "Shift-JIS (CP932)",
            TextEncoding::Gbk => "GBK (CP936)",
            TextEncoding::Big5 => "Big5 (CP950)",
            TextEncoding::Windows1252 => "ANSI (Windows-1252)",
        }
    }

    /// 옛 (한 바이트·두 바이트) 인코딩이면 encoding_rs 의 인코딩
    fn legacy(self) -> Option<&'static encoding_rs::Encoding> {
        match self {
            TextEncoding::EucKr => Some(encoding_rs::EUC_KR),
            TextEncoding::ShiftJis => Some(encoding_rs::SHIFT_JIS),
            TextEncoding::Gbk => Some(encoding_rs::GBK),
            TextEncoding::Big5 => Some(encoding_rs::BIG5),
            TextEncoding::Windows1252 => Some(encoding_rs::WINDOWS_1252),
            _ => None,
        }
    }
}

/// 파일 내용 → (텍스트, 인코딩). BOM 이 있으면 그대로, 없으면 UTF-8 인지 보고 아니면 옛 인코딩을 알아냅니다.
pub fn decode(bytes: &[u8]) -> (String, TextEncoding) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(rest).into_owned(), TextEncoding::Utf8Bom);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let (cow, _) = encoding_rs::UTF_16LE.decode_without_bom_handling(rest);
        return (cow.into_owned(), TextEncoding::Utf16Le);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let (cow, _) = encoding_rs::UTF_16BE.decode_without_bom_handling(rest);
        return (cow.into_owned(), TextEncoding::Utf16Be);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_owned(), TextEncoding::Utf8),
        Err(_) => decode_legacy(bytes),
    }
}

/// BOM 도 없고 UTF-8 도 아닌 파일: 어느 옛 인코딩인지 내용으로 알아냅니다.
/// 예전에는 무조건 CP949 로 읽어서, 일본어 · 중국어 · 서유럽 파일을 깨진 채로 저장할 수 있었습니다.
/// 짧은 글은 내용만으로 가리기 어려우므로 화면 언어를 힌트로 줍니다 (한국어면 짧은 CP949 메모를 바르게).
fn decode_legacy(bytes: &[u8]) -> (String, TextEncoding) {
    let hint: Option<&[u8]> = match crate::i18n::lang() {
        crate::i18n::Lang::Ko => Some(b"kr"),
        crate::i18n::Lang::Ja => Some(b"jp"),
        crate::i18n::Lang::Zh => Some(b"cn"),
        crate::i18n::Lang::En => None,
    };
    detect_legacy(bytes, hint)
}

/// `hint`: 이 글이 왔을 법한 나라의 최상위 도메인 (`kr`, `jp` ...)
fn detect_legacy(bytes: &[u8], hint: Option<&[u8]>) -> (String, TextEncoding) {
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    let guess = detector.guess(hint, false);
    let ours = TextEncoding::ALL
        .into_iter()
        .find(|e| e.legacy() == Some(guess) || (guess == encoding_rs::GB18030 && *e == TextEncoding::Gbk));
    let (cow, _) = guess.decode_without_bom_handling(bytes);
    // 목록에 없는 인코딩(키릴 등)은 내용은 바르게 읽고, 저장은 UTF-8 로 (원래 인코딩으로 되돌릴 수 없으니 깨지지 않게)
    (cow.into_owned(), ours.unwrap_or(TextEncoding::Utf8))
}

/// 인코딩할 수 없는 문자가 있으면 `None`
pub fn encode(text: &str, encoding: TextEncoding) -> Option<Vec<u8>> {
    match encoding {
        TextEncoding::Utf8 => Some(text.as_bytes().to_vec()),
        TextEncoding::Utf8Bom => {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(text.as_bytes());
            Some(v)
        }
        TextEncoding::Utf16Le => {
            let mut v = vec![0xFF, 0xFE];
            for u in text.encode_utf16() {
                v.extend_from_slice(&u.to_le_bytes());
            }
            Some(v)
        }
        TextEncoding::Utf16Be => {
            let mut v = vec![0xFE, 0xFF];
            for u in text.encode_utf16() {
                v.extend_from_slice(&u.to_be_bytes());
            }
            Some(v)
        }
        legacy => {
            let enc = legacy.legacy()?;
            let (bytes, _, had_errors) = enc.encode(text);
            (!had_errors).then(|| bytes.into_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_legacy_encodings() {
        let samples: [(&str, TextEncoding); 5] = [
            ("안녕하세요. 오늘 회의 내용을 정리합니다. 예산과 일정을 다시 확인해 주세요.", TextEncoding::EucKr),
            (
                "こんにちは。今日の会議の内容をまとめます。予算と日程をもう一度確認してください。",
                TextEncoding::ShiftJis,
            ),
            ("你好。今天整理一下会议内容。请再确认一下预算和日程安排，谢谢。", TextEncoding::Gbk),
            ("你好。今天整理一下會議內容。請再確認一下預算和日程安排，謝謝。", TextEncoding::Big5),
            ("Café menu: crème brûlée, naïve façade, déjà vu, piñata and résumé.", TextEncoding::Windows1252),
        ];
        let detect = |text: &str, enc: TextEncoding, hint: Option<&[u8]>| {
            let bytes = encode(text, enc).unwrap();
            let (decoded, found) = detect_legacy(&bytes, hint);
            assert_eq!((decoded.as_str(), found), (text, enc), "{enc:?} / {hint:?}");
        };
        for (text, enc) in samples {
            // 화면 언어 힌트 없이도
            detect(text, enc, None);
        }
        // 한국어 화면: 짧은 CP949 메모도, 다른 나라 인코딩도
        detect("첫 줄\r\n둘째 줄", TextEncoding::EucKr, Some(b"kr"));
        for (text, enc) in samples.into_iter().filter(|(_, e)| *e != TextEncoding::Gbk) {
            detect(text, enc, Some(b"kr"));
        }
        detect(samples[2].0, TextEncoding::Gbk, Some(b"cn"));
        detect(samples[1].0, TextEncoding::ShiftJis, Some(b"jp"));
    }

    #[test]
    fn roundtrip_encodings() {
        let text = "안녕하세요 Hello\n둘째 줄";
        // 다른 언어의 옛 인코딩은 한글을 담을 수 없으므로 detects_legacy_encodings 에서 따로 시험
        for enc in TextEncoding::ALL.into_iter().filter(|e| e.legacy().is_none() || *e == TextEncoding::EucKr) {
            let bytes = encode(text, enc).unwrap();
            let (decoded, detected) = decode(&bytes);
            assert_eq!(decoded, text, "{enc:?}");
            assert_eq!(detected, enc);
        }
    }

    #[test]
    fn euc_kr_rejects_unmappable() {
        assert!(encode("이모지 😀", TextEncoding::EucKr).is_none());
    }
}
