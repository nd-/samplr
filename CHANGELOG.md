# Changelog

All notable changes to samplr are written here, after
[Keep a Changelog](https://keepachangelog.com/en/1.0.0/). No version has been released yet.

## [Unreleased]

### Added

#### Library

- Scan a sample folder and its subfolders (WAV, AIFF, FLAC, MP3, OGG, M4A) into a SQLite
  database, in the system's data folder; a new scan updates without duplicates and keeps tags.
- Follow the scanned folders live: added, renamed, moved and deleted files follow, renames keep
  the tags; the changes made while samplr was closed are caught up with at startup.
- An unplugged disk loses nothing: its samples and their tags stay until it comes back.
- Hidden files and folders below a scanned folder (a disk's trash, `._*` files left by macOS)
  are never samples.
- Remove a scanned folder: its samples leave the list but keep their tags, back if the folder is
  scanned again. The audio files are never touched.
- Waveforms computed once and cached, in the background for the whole library.
- Loudness of each sample (integrated LUFS and peak), measured with its waveform.
- Exact length of MP3 and OGG files, without the encoder's delay and padding.

#### Tags, favorites and collections

- Tags, lowercase, added and removed for a whole selection; a tag no sample carries goes.
- Favorites, and a filter showing them only.
- Collections: samples gathered by hand, by the row menu or by dropping rows or files on them;
  names lowercase, as tags.
- A palette of colours, editable, for tags and collections.

#### Tempo, key and kind of sound

- Tempo and key from the file name, from the WAV files' ACID chunk, else the tempo from a loop's
  length, or detected from its sound as a last resort; set by hand, or chosen by method with
  Detect BPM.
- Loop or one-shot, from the ACID chunk, the words of the path or the tempo, switchable by hand.
- A loop's length in bars.
- The kind of sound in two levels (Drums › Kick), worked out from the path, after LANDR's and
  Splice's categories; set or searched with a type picker.

#### Searching

- One search box: words found in the path or the tags, `#tag`, and the filters `bpm:120`,
  `bpm:118-124`, `key:Am`, `loop:yes`, `type:kick`; accents and case ignored.
- The same syntax for `samplr search` on the command line.

#### Interface

- A window built with Tauri: the sample list, the collections and tags on the left, an Info
  panel on the right with the file's own metadata, a playback bar at the bottom.
- Preview played by samplr itself: the arrow keys browse and play, Space pauses a loop or plays
  a one-shot again, L switches the loop, a click on the waveform plays from that point.
- Pitch in semitones and cents and a target tempo, changed one without the other (Signalsmith
  Stretch) or together as a tape; a volume slider.
- Drag samples into a DAW or a file manager, offered as copies.
- Multiple selection with Ctrl, Shift and Ctrl+A; a double-click edits the tempo, key, loop or
  type of the whole selection.
- Sort by any column, in natural order; resize, hide and show the columns.
- Rename a sample's file, move it to another folder, convert it with ffmpeg (WAV, AIFF, FLAC,
  MP3, OGG, optionally to mono, the copy keeping the tags and collections), move it to the
  system's trash, or show it in the Finder or the file manager.
- Add a folder with a progress bar; the Splice folders found are offered at the first launch.
- Six colour themes in light and dark, named after music genres, and a System one.
- The keyboard shortcuts listed in Settings, with the Mac's keys on a Mac.
- The window's size, the theme, the columns and the playback settings remembered between
  launches.

#### Command line

- `samplr scan`, `list`, `search`, `tag`, `untag`, `tags`, `fav`, `unfav`, `folders`,
  `remove-folder`, `watch`, `play`, `wave` and `waveforms`; `--json` for scripts on any command.

[Unreleased]: https://github.com/nd-/samplr/commits/main
