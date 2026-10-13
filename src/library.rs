//! Spotify's catalogue, read through librespot's metadata: for now, the tracks of an album.
//!
//! Only [`crate::player`] uses this module, with its own session: librespot's types stay
//! inside the crate.

use librespot::core::{Error, Session, SpotifyUri};
use librespot::metadata::album::Disc;
use librespot::metadata::{Album, Metadata};

/// The tracks of `album` in listening order, as URIs ready for the queue.
pub(crate) async fn album_tracks(session: &Session, album: &SpotifyUri) -> Result<Vec<String>, Error> {
    let album = Album::get(session, album).await?;
    Ok(listening_order(&album.discs))
}

/// The tracks of an album's discs in listening order: disc 1 before disc 2, and on each
/// disc the order Spotify gives. Entries that are neither tracks nor episodes are left
/// out, so that they cannot stop the rest of the queue.
fn listening_order(discs: &[Disc]) -> Vec<String> {
    let mut discs: Vec<&Disc> = discs.iter().collect();
    // Spotify already lists the discs in order: the stable sort only guards against
    // a different one, and keeps the given order for discs with the same number.
    discs.sort_by_key(|disc| disc.number);
    discs
        .iter()
        .flat_map(|disc| disc.tracks.iter())
        .filter(|uri| matches!(uri, SpotifyUri::Track { .. } | SpotifyUri::Episode { .. }))
        .map(SpotifyUri::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use librespot::metadata::track::Tracks;

    use super::*;

    fn disc(number: i32, tracks: &[&str]) -> Disc {
        Disc {
            number,
            name: String::new(),
            tracks: Tracks(tracks.iter().map(|uri| SpotifyUri::from_uri(uri).unwrap()).collect()),
        }
    }

    const ONE: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";
    const TWO: &str = "spotify:track:6rqhFgbbKwnb9MLmUQDhG6";
    const THREE: &str = "spotify:track:7GhIk7Il098yCjg4BQjzvb";
    const FOUR: &str = "spotify:track:0VjIjW4GlUZAMYd2vXMi3b";

    #[test]
    fn the_tracks_of_one_disc_keep_their_order() {
        assert_eq!(listening_order(&[disc(1, &[ONE, TWO, THREE])]), [ONE, TWO, THREE]);
    }

    #[test]
    fn the_discs_are_played_by_number() {
        let discs = [disc(2, &[THREE, FOUR]), disc(1, &[ONE, TWO])];
        assert_eq!(listening_order(&discs), [ONE, TWO, THREE, FOUR]);
    }

    #[test]
    fn discs_with_the_same_number_keep_the_given_order() {
        let discs = [disc(1, &[ONE]), disc(1, &[TWO]), disc(0, &[THREE])];
        assert_eq!(listening_order(&discs), [THREE, ONE, TWO]);
    }

    #[test]
    fn episodes_stay_and_other_entries_are_left_out() {
        let episode = "spotify:episode:512ojhOuo1ktJprKbVcKyQ";
        let artist = "spotify:artist:0gxyHStUsqpMadRV0Di1Qt";
        assert_eq!(listening_order(&[disc(1, &[ONE, artist, episode, TWO])]), [ONE, episode, TWO]);
    }

    #[test]
    fn an_album_without_tracks_gives_an_empty_queue() {
        assert!(listening_order(&[]).is_empty());
        assert!(listening_order(&[disc(1, &[])]).is_empty());
    }

    #[test]
    fn the_uris_come_out_in_canonical_form() {
        let named = "spotify:user:someone:track:4uLU6hMCjMI75M1A2tKUQC";
        assert_eq!(listening_order(&[disc(1, &[named])]), [ONE]);
    }
}
