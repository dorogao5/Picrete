use rand::Rng;
use sha2::{Digest, Sha256};

use crate::db::types::CourseRole;

const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub(crate) fn generate_invite_code(course_slug: &str, role: CourseRole) -> String {
    let role_prefix = match role {
        CourseRole::Teacher => "TEACH",
        CourseRole::Student => "STUD",
    };

    let normalized_slug = course_slug
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(6)
        .collect::<String>()
        .to_uppercase();

    let random = generate_suffix(8);
    format!("{}-{}-{}", normalized_slug, role_prefix, random)
}

pub(crate) fn hash_invite_code(invite_code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(invite_code.as_bytes());
    hex::encode(hasher.finalize())
}

fn generate_suffix(len: usize) -> String {
    let mut rng = rand::thread_rng();
    let mut output = String::with_capacity(len);
    for _ in 0..len {
        let index = rng.gen_range(0..ALPHABET.len());
        output.push(ALPHABET[index] as char);
    }
    output
}
