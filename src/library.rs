//! What is in a folder, and in what order.
//!
//! A photo viewer is a walk over directories, so this is deliberately the
//! whole of the model: one directory at a time, read when it is reached, with
//! the folders kept alongside the pictures so the walk can go further in.
//! Nothing here draws, and nothing here decodes — reading a directory is cheap
//! and reading a photograph is not, and the two must not be the same act.
//!
//! The order is the user's, not the disk's. `readdir` answers in whatever
//! order the filesystem finds convenient, which for a folder of photographs is
//! no order at all, so a listing is always sorted before it is shown.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The file endings this reads, which are the ones `image` is built for here.
///
/// Matched without case, because a camera that writes `.JPG` and a phone that
/// writes `.jpg` are both photographs and neither is a mistake.
pub const ENDINGS: &[&str] = &[
    "png", "jpg", "jpeg", "jfif", "gif", "webp", "bmp", "tif", "tiff", "ico", "pnm", "pbm", "pgm",
    "ppm", "qoi", "tga", "dds", "hdr", "exr", "avif",
];

pub fn is_photograph(path: &Path) -> bool {
    path.extension()
        .and_then(|ending| ending.to_str())
        .is_some_and(|ending| {
            ENDINGS
                .iter()
                .any(|known| ending.eq_ignore_ascii_case(known))
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Folder,
    Photograph,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: Kind,
    pub bytes: u64,
    pub changed: Option<SystemTime>,
}

impl Entry {
    pub fn is_folder(&self) -> bool {
        self.kind == Kind::Folder
    }
}

/// The nine orders a listing can be in.
///
/// Folders first in every one of them, as they are in the shell's own chooser:
/// a folder is a way further in rather than a thing being sorted, and a walk
/// whose doors moved about between orders would be a different room each time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    #[default]
    Name,
    NameReversed,
    Newest,
    Oldest,
    Largest,
    Smallest,
}

impl Order {
    pub const ALL: [Order; 6] = [
        Order::Name,
        Order::NameReversed,
        Order::Newest,
        Order::Oldest,
        Order::Largest,
        Order::Smallest,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Order::Name => "Name",
            Order::NameReversed => "Name, backwards",
            Order::Newest => "Newest first",
            Order::Oldest => "Oldest first",
            Order::Largest => "Largest first",
            Order::Smallest => "Smallest first",
        }
    }
}

/// One directory, read.
#[derive(Debug, Clone, Default)]
pub struct Folder {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    /// Set when the directory itself could not be read, as opposed to being
    /// readable and empty. The two say very different things to somebody
    /// looking at an empty grid, so they are not the same state.
    pub unreadable: bool,
}

impl Folder {
    /// Read one directory. Never fails: a directory that cannot be opened is a
    /// folder with nothing in it that says so.
    pub fn read(path: impl AsRef<Path>, order: Order, hidden: bool) -> Folder {
        let path = path.as_ref().to_path_buf();
        let Ok(reading) = std::fs::read_dir(&path) else {
            return Folder {
                path,
                entries: Vec::new(),
                unreadable: true,
            };
        };

        let mut entries = Vec::new();
        for found in reading.flatten() {
            let Ok(name) = found.file_name().into_string() else {
                // A name that is not UTF-8 cannot be drawn, and a row that
                // cannot say what it is is worse than one that is not there.
                continue;
            };
            if !hidden && name.starts_with('.') {
                continue;
            }
            // Followed rather than tested, so a symlink to a folder is a way
            // further in and a symlink to a photograph is a photograph.
            let Ok(about) = found.path().metadata() else {
                continue;
            };
            let kind = if about.is_dir() {
                Kind::Folder
            } else if is_photograph(&found.path()) {
                Kind::Photograph
            } else {
                continue;
            };
            entries.push(Entry {
                path: found.path(),
                name,
                kind,
                bytes: about.len(),
                changed: about.modified().ok(),
            });
        }

        sort(&mut entries, order);
        Folder {
            path,
            entries,
            unreadable: false,
        }
    }

    /// Where in the listing this path is, if it is in it at all.
    pub fn position(&self, path: &Path) -> Option<usize> {
        self.entries.iter().position(|entry| entry.path == path)
    }

    pub fn photographs(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| !entry.is_folder())
            .count()
    }

    /// The next photograph at or after `from`, walking `step` at a time and
    /// coming round the ends.
    ///
    /// Folders are stepped over rather than stopped at: once a picture is
    /// being looked at, the next one is the next *picture*, and a directory
    /// sitting between two of them is not somewhere the viewer can go.
    /// Answers `None` where there is no other photograph to reach, which
    /// includes the one-photograph folder — a step that lands back where it
    /// started is not a step.
    pub fn step(&self, from: usize, step: isize) -> Option<usize> {
        let count = self.entries.len();
        if count == 0 || self.photographs() <= 1 {
            return None;
        }
        let mut at = from;
        for _ in 0..count {
            at = (at as isize + step).rem_euclid(count as isize) as usize;
            if !self.entries[at].is_folder() {
                return (at != from).then_some(at);
            }
        }
        None
    }
}

pub fn sort(entries: &mut [Entry], order: Order) {
    entries.sort_by(|left, right| {
        // Folders before pictures, whatever the order is otherwise.
        left.is_folder()
            .cmp(&right.is_folder())
            .reverse()
            .then_with(|| match order {
                Order::Name => natural(&left.name, &right.name),
                Order::NameReversed => natural(&left.name, &right.name).reverse(),
                Order::Newest => right.changed.cmp(&left.changed),
                Order::Oldest => left.changed.cmp(&right.changed),
                Order::Largest => right.bytes.cmp(&left.bytes),
                Order::Smallest => left.bytes.cmp(&right.bytes),
            })
            // Two photographs taken in the same second, or written to the same
            // length, still have to come out in one order every time the
            // folder is read, or the grid reshuffles itself for no reason.
            .then_with(|| natural(&left.name, &right.name))
    });
}

/// Compare two names the way somebody reading them would.
///
/// `IMG_9.jpg` comes before `IMG_10.jpg`, which plain byte order gets exactly
/// backwards — and a camera's own numbering is the order a folder of
/// photographs is nearly always meant to be in, so getting this wrong reorders
/// the one thing the user is surest about.
fn natural(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let mut left = left.chars().peekable();
    let mut right = right.chars().peekable();
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(one), Some(other)) => {
                if one.is_ascii_digit() && other.is_ascii_digit() {
                    let one = run_of_digits(&mut left);
                    let other = run_of_digits(&mut right);
                    // Compared as numbers, so any amount of leading nought is
                    // only a tie-break rather than a different number.
                    match one
                        .trim_start_matches('0')
                        .len()
                        .cmp(&other.trim_start_matches('0').len())
                    {
                        Ordering::Equal => match one
                            .trim_start_matches('0')
                            .cmp(other.trim_start_matches('0'))
                        {
                            Ordering::Equal => match one.len().cmp(&other.len()) {
                                Ordering::Equal => {}
                                uneven => return uneven,
                            },
                            uneven => return uneven,
                        },
                        uneven => return uneven,
                    }
                } else {
                    let folded = one
                        .to_lowercase()
                        .cmp(other.to_lowercase())
                        .then_with(|| one.cmp(&other));
                    if folded != Ordering::Equal {
                        return folded;
                    }
                    left.next();
                    right.next();
                }
            }
        }
    }
}

fn run_of_digits(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut digits = String::new();
    while let Some(digit) = chars.peek().copied() {
        if !digit.is_ascii_digit() {
            break;
        }
        digits.push(digit);
        chars.next();
    }
    digits
}

/// Where to open when nothing said.
///
/// The user's pictures if they have such a folder, and their home if they do
/// not — never the working directory, which for an application launched off a
/// shell's start screen is wherever the shell happened to be.
pub fn default_folder() -> PathBuf {
    if let Some(pictures) = xdg_pictures() {
        if pictures.is_dir() {
            return pictures;
        }
    }
    home().unwrap_or_else(|| PathBuf::from("/"))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// `XDG_PICTURES_DIR` out of the user's directory record, falling back to the
/// English default the specification names.
///
/// Read rather than guessed because the folder is localised: a German desktop
/// calls it `Bilder`, and a viewer that opened `~/Pictures` on it would open
/// nothing.
fn xdg_pictures() -> Option<PathBuf> {
    let home = home()?;
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let record = std::fs::read_to_string(config.join("user-dirs.dirs")).unwrap_or_default();
    for line in record.lines() {
        let line = line.trim();
        let Some(value) = line.strip_prefix("XDG_PICTURES_DIR=") else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        let Some(rest) = value.strip_prefix("$HOME/") else {
            if value.starts_with('/') {
                return Some(PathBuf::from(value));
            }
            continue;
        };
        return Some(home.join(rest));
    }
    Some(home.join("Pictures"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_camera_s_own_numbering_reads_in_order() {
        let mut names = ["IMG_10.jpg", "IMG_9.jpg", "IMG_100.jpg", "IMG_1.jpg"];
        names.sort_by(|left, right| natural(left, right));
        assert_eq!(
            names,
            ["IMG_1.jpg", "IMG_9.jpg", "IMG_10.jpg", "IMG_100.jpg"]
        );
    }

    #[test]
    fn leading_noughts_are_only_a_tie_break() {
        assert_eq!(natural("007.png", "7.png"), std::cmp::Ordering::Greater);
        assert_eq!(natural("08.png", "9.png"), std::cmp::Ordering::Less);
    }

    #[test]
    fn every_ending_is_lower_case_so_the_match_is_the_only_folding() {
        for ending in ENDINGS {
            assert_eq!(*ending, ending.to_ascii_lowercase(), "{ending}");
        }
    }

    #[test]
    fn a_photograph_is_recognised_whatever_the_case() {
        assert!(is_photograph(Path::new("/a/B.JPG")));
        assert!(is_photograph(Path::new("/a/b.jpeg")));
        assert!(!is_photograph(Path::new("/a/b.txt")));
        assert!(!is_photograph(Path::new("/a/b")));
    }

    fn photo(name: &str) -> Entry {
        Entry {
            path: PathBuf::from(name),
            name: name.to_string(),
            kind: Kind::Photograph,
            bytes: 0,
            changed: None,
        }
    }

    fn folder(name: &str) -> Entry {
        Entry {
            kind: Kind::Folder,
            ..photo(name)
        }
    }

    #[test]
    fn folders_lead_in_every_order() {
        for order in Order::ALL {
            let mut entries = vec![photo("b.png"), folder("z"), photo("a.png"), folder("a")];
            sort(&mut entries, order);
            assert!(entries[0].is_folder(), "{order:?}");
            assert!(entries[1].is_folder(), "{order:?}");
            assert!(!entries[2].is_folder(), "{order:?}");
        }
    }

    #[test]
    fn stepping_walks_past_folders_and_comes_round() {
        let listing = Folder {
            path: PathBuf::new(),
            entries: vec![folder("d"), photo("a.png"), folder("e"), photo("b.png")],
            unreadable: false,
        };
        assert_eq!(listing.step(1, 1), Some(3));
        assert_eq!(listing.step(3, 1), Some(1));
        assert_eq!(listing.step(1, -1), Some(3));
    }

    #[test]
    fn one_photograph_has_nowhere_to_step() {
        let listing = Folder {
            path: PathBuf::new(),
            entries: vec![folder("d"), photo("a.png")],
            unreadable: false,
        };
        assert_eq!(listing.step(1, 1), None);
    }

    #[test]
    fn an_unreadable_folder_is_not_an_empty_one() {
        let listing = Folder::read("/no/such/place/at/all", Order::Name, false);
        assert!(listing.unreadable);
        assert!(listing.entries.is_empty());
    }
}
