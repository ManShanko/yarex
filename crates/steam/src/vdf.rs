// Basic vdf/acf parser for Steam files
#![allow(non_snake_case, dead_code)]
use std::path::PathBuf;
use std::collections::HashMap;

pub struct AppManifest {
    pub appid: u64,
    pub name: String,
    pub installdir: String,
//    LastUpdated:
    pub SizeOnDisk: u64,
}

pub fn read_app_manifest(s: &str) -> Result<AppManifest, &'static str> {
    loop {
        if let ValueKind::Object(obj) = parse_acf(s) {
            if let Some(ValueKind::Object(app)) = obj.get("AppState") {
                let appid = match app.get("appid") {
                    Some(ValueKind::Number(t)) => t,
                    _ => break,
                };
                let name = match app.get("name") {
                    Some(ValueKind::Text(t)) => t,
                    _ => break,
                };
                let dir = match app.get("installdir") {
                    Some(ValueKind::Text(t)) => t,
                    _ => break,
                };
                let size = match app.get("SizeOnDisk") {
                    Some(ValueKind::Number(t)) => t,
                    _ => break,
                };
                return Ok(AppManifest {
                    appid:      *appid,
                    name:       name.to_owned(),
                    installdir: dir.to_owned(),
                    SizeOnDisk: *size,
                });
            }
        }
    }
    Err("received unexpected type")
}

pub struct LibraryFolders {
    pub folders: Vec<PathBuf>,
}

pub fn get_library_folders(s: &str) -> Result<LibraryFolders, &'static str> {
    let obj = match parse_acf(s) {
        ValueKind::Object(t) => t,
        _ => return Err("received unexpected type"),
    };

    let mut lf = LibraryFolders {
        folders: Vec::<PathBuf>::new(),
    };
    match obj.get("LibraryFolders") {
        Some(ValueKind::Object(t)) => {
            for entry in t {
                let field = entry.0;
                field.parse::<u8>().map(|_| {
                    if let ValueKind::Text(s) = entry.1 {
                        lf.folders.push(PathBuf::from(fix_slashes(s)));
                    }
                }).ok();
            }
        },
        _ => return Err("missing field LibraryFolders in Object"),
    }
    Ok(lf)
}

fn fix_slashes(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut t = false;
    let mut out = Vec::<char>::new();
    for c in bytes {
        let c = *c as char;
        if c == '\\' {
            if !t {
                out.push(c);
            }
            t = true;
        }
        else {
            out.push(c);
            t = false;
        }
    }
    out.into_iter().collect()
}

#[derive(Debug, PartialEq)]
enum ValueKind {
    Object(HashMap<String, ValueKind>),
    Text(String),
    Number(u64),
}

struct Parser {
    st: ParserState,
    c: char,
    buffer: Vec<char>,
    key: String,
    obj: Vec<(String, HashMap<String, ValueKind>)>,
}

impl Parser {
    fn parse_acf(s: &str) -> ValueKind {
        let b = s.as_bytes();
        let mut p = Parser {
            st: ParserState::ParseKey,
            c: 0 as char,
            buffer: Vec::<char>::with_capacity(64),
            key: String::new(),
            obj: vec![("".to_string(), HashMap::new())],
        };
        for c in b {
            p.parse(*c as char);
        }
        ValueKind::Object(p.obj.pop().unwrap().1)
    }

    fn parse_key(&mut self) {
        if self.c == '"' {
            self.st = ParserState::Key
        }
        else if self.c == '}' {
            let j = self.obj.pop().unwrap();
            self.obj.last_mut().unwrap().1.insert(j.0, ValueKind::Object(j.1));
        }
    }

    fn key(&mut self) {
        if self.c != '"' {
            self.buffer.push(self.c);
        }
        else {
            self.st = ParserState::ParseValue;
            self.key = self.buffer.iter().collect();
            self.buffer.clear();
        }
    }

    fn parse_value(&mut self) {
        if self.c == '"' {
            self.st = ParserState::Value;
        }
        else if self.c == '{' {
            self.st = ParserState::ParseKey;
            let s = self.key.to_owned();
            let obj = HashMap::new();
            self.obj.push((s, obj));
        }
    }

    fn value(&mut self) {
        if self.c != '"' {
            self.buffer.push(self.c);
        }
        else {
            self.st = ParserState::ParseKey;
            let s = self.buffer.iter().collect::<String>();
            self.buffer.clear();
            match s.parse::<u64>() {
                Ok(num) => {
                    self.obj.last_mut().unwrap().1.insert(self.key.to_owned(), ValueKind::Number(num));
                },
                _ => {
                    self.obj.last_mut().unwrap().1.insert(self.key.to_owned(), ValueKind::Text(s));
                },
            }
        }
    }

    fn parse(&mut self, c: char) {
        self.c = c;
        match &self.st {
            ParserState::ParseKey   => self.parse_key(),
            ParserState::Key     => self.key(),
            ParserState::ParseValue => self.parse_value(),
            ParserState::Value   => self.value(),
        }
    }
}

enum ParserState {
    ParseKey,
    Key,
    ParseValue,
    Value,
}

fn parse_acf(vdf: &str) -> ValueKind {
    Parser::parse_acf(vdf)
}

fn parse_vdf(vdf: &str) -> ValueKind {
    parse_acf(vdf)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_libraryfolders() {
        let vdf = r#"
            "LibraryFolders"
            {
                "1"     "D:\\SteamLibrary"
                "2"     "E:\\SteamLibrary"
            }
        "#;

        let obj = parse_vdf(vdf);

        if let ValueKind::Object(obj) = obj {
            if let Some(ValueKind::Object(lib)) = obj.get("LibraryFolders") {
                assert_eq!(lib.get("1").unwrap(), &ValueKind::Text(r"D:\\SteamLibrary".into()));
                assert_eq!(lib.get("2").unwrap(), &ValueKind::Text(r"E:\\SteamLibrary".into()));

                return;
            }
        }

        panic!();
    }

    #[test]
    fn parse_appmanifest() {
        let acf = r#"
            "AppState"
            {
                "appid"     "55500"
                "name"      "Billy's Big Game: The 2nd"
                "installdir"        "Billys Big Game The 2nd"
                "LastUpdated"       "1610000000"
                "SizeOnDisk"        "111111111111"
                "buildid"           "456"
                "InstalledDepots"
                {
                    "55501"
                    {
                    }
                    "55502"
                    {
                        "SizeOnDisk"        "111"
                    }
                }
                "UserConfig"
                {
                    "language"      "english"
                }
            }
        "#;

        if let ValueKind::Object(obj) = parse_vdf(acf) {
            if let Some(ValueKind::Object(app)) = obj.get("AppState") {
                assert_eq!(app.get("name"), Some(&ValueKind::Text("Billy's Big Game: The 2nd".into())));
                if let Some(ValueKind::Object(user_config)) = app.get("UserConfig") {
                    assert_eq!(user_config.get("language"), Some(&ValueKind::Text("english".into())));
                } else { panic!(); }

                if let Some(ValueKind::Object(install_depots)) = app.get("InstalledDepots") {
                    //assert!(install_depots.get("55501").is_empty());

                    if let Some(ValueKind::Object(depot)) = install_depots.get("55502") {
                        assert_eq!(depot.get("SizeOnDisk"), Some(&ValueKind::Number(111)));
                    } else { panic!(); }
                } else { panic!(); }

                let app = read_app_manifest(acf).unwrap();
                assert_eq!(app.appid, 55500);
                assert_eq!(app.name, "Billy's Big Game: The 2nd".to_string());
                assert_eq!(app.installdir, "Billys Big Game The 2nd".to_string());

                return;
            }
        }
        panic!();
    }
}



