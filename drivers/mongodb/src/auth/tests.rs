use super::*;

fn conversation(mechanism: Mechanism, password: &str, nonce: &str) -> Scram {
    Scram { mechanism, user: "user".into(), password: password.into(), nonce: nonce.into() }
}

#[test]
fn scram_sha_1_matches_rfc_5802() {
    let scram = conversation(Mechanism::Sha1, "pencil", "fyko+d2lbbFgONRv9qkxdawL");
    assert_eq!(scram.first(), "n,,n=user,r=fyko+d2lbbFgONRv9qkxdawL");
    let proof = scram.answer("r=fyko+d2lbbFgONRv9qkxdawL3rfcNHYJY1ZVvWVs7j,s=QSXCR+Q6sek8bf92,i=4096").unwrap();
    assert_eq!(proof.message, "c=biws,r=fyko+d2lbbFgONRv9qkxdawL3rfcNHYJY1ZVvWVs7j,p=v0X8v3Bz2T0CJGbJQyF0X+HI4Ts=");
    assert_eq!(STANDARD.encode(proof.server_signature), "rmF9pqV8S7suAoZWja4dJRkFsKQ=");
}

#[test]
fn scram_sha_256_matches_rfc_7677() {
    let scram = conversation(Mechanism::Sha256, "pencil", "rOprNGfwEbeRWgbNEkqO");
    let server_first = "r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096";
    let proof = scram.answer(server_first).unwrap();
    assert_eq!(
        proof.message,
        "c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,p=dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ="
    );
    assert_eq!(STANDARD.encode(proof.server_signature), "6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=");
}

#[test]
fn mongodb_digests_the_password_for_sha_1_and_prepares_it_for_sha_256() {
    let sha1 = Scram::new(Mechanism::Sha1, "user", "pencil", "n".into()).unwrap();
    assert_eq!(sha1.password, "1c33006ec1ffd90f9cadcbcc0e118200");
    let sha256 = Scram::new(Mechanism::Sha256, "user", "I\u{AD}X", "n".into()).unwrap();
    assert_eq!(sha256.password, "IX", "SASLprep maps soft hyphens to nothing");
}

#[test]
fn a_server_that_does_not_extend_the_nonce_or_skimps_on_iterations_is_refused() {
    let scram = conversation(Mechanism::Sha256, "pencil", "abc");
    assert_eq!(scram.answer("r=xyz1,s=QSXCR+Q6sek8bf92,i=4096").unwrap_err().kind(), Some("authentication"));
    assert!(scram.answer("r=abcdef,s=QSXCR+Q6sek8bf92,i=10").is_err());
}

#[test]
fn the_mechanism_is_named_or_chosen_from_what_the_server_offers() {
    assert_eq!(Mechanism::named("scram-sha-1").unwrap(), Some(Mechanism::Sha1));
    assert_eq!(Mechanism::named("").unwrap(), None);
    assert!(Mechanism::named("MONGODB-X509").is_err());
    assert_eq!(Mechanism::offered(&doc! { "saslSupportedMechs": ["SCRAM-SHA-1"] }), Mechanism::Sha1);
    assert_eq!(Mechanism::offered(&doc! { "saslSupportedMechs": ["SCRAM-SHA-1", "SCRAM-SHA-256"] }), Mechanism::Sha256);
    assert_eq!(Mechanism::offered(&doc! {}), Mechanism::Sha256);
    assert_eq!(nonce().unwrap().len(), 32);
}
