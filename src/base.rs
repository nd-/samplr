// Base SQLite de samplr : la liste des samples connus et leurs tags.
// Les tags vivent ici, jamais dans les fichiers audio.

use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use rusqlite::{Connection, params};

use crate::InfoSample;

/// Emplacement du fichier de la base : le dossier de données du système
/// (`~/.local/share/samplr` sous Linux, `~/Library/Application Support/samplr`
/// sur Mac). La variable d'environnement `SAMPLR_BASE` permet d'en utiliser
/// une autre, pour faire des essais sans toucher à la vraie.
fn chemin_base() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(chemin) = env::var_os("SAMPLR_BASE") {
        return Ok(PathBuf::from(chemin));
    }
    let dossier = dirs::data_dir()
        .ok_or("dossier de données du système introuvable")?
        .join("samplr");
    fs::create_dir_all(&dossier)?;
    Ok(dossier.join("samplr.db"))
}

/// Ouvre la base (la crée au premier lancement) et s'assure que les tables existent.
pub fn ouvrir() -> Result<Connection, Box<dyn Error>> {
    let base = Connection::open(chemin_base()?)?;

    // SQLite ne vérifie les clés étrangères que si on le lui demande,
    // et ce réglage est à refaire à chaque ouverture.
    base.execute_batch(
        "PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS fichiers (
            id             INTEGER PRIMARY KEY,
            chemin         TEXT NOT NULL UNIQUE,
            duree_secondes REAL,
            frequence      INTEGER,
            canaux         INTEGER,
            bits           INTEGER
        );

        CREATE TABLE IF NOT EXISTS tags (
            id  INTEGER PRIMARY KEY,
            nom TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS fichiers_tags (
            fichier_id INTEGER NOT NULL REFERENCES fichiers(id) ON DELETE CASCADE,
            tag_id     INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            PRIMARY KEY (fichier_id, tag_id)
        );",
    )?;

    Ok(base)
}

/// Ajoute un sample à la base, ou met ses infos à jour s'il y est déjà.
pub fn enregistrer(base: &Connection, info: &InfoSample) -> Result<(), Box<dyn Error>> {
    // Le chemin est stocké en texte : un nom de fichier qui n'est pas de
    // l'UTF-8 valide (possible sous Linux) ne pourrait pas être retrouvé.
    let chemin = info
        .chemin
        .to_str()
        .ok_or("nom de fichier avec des caractères invalides")?;

    // `ON CONFLICT … DO UPDATE` met à jour la ligne existante au lieu de la
    // remplacer : le sample garde son `id`, donc ses tags.
    base.execute(
        "INSERT INTO fichiers (chemin, duree_secondes, frequence, canaux, bits)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (chemin) DO UPDATE SET
             duree_secondes = excluded.duree_secondes,
             frequence      = excluded.frequence,
             canaux         = excluded.canaux,
             bits           = excluded.bits",
        params![
            chemin,
            info.duree_secondes,
            info.frequence,
            info.canaux,
            info.bits
        ],
    )?;
    Ok(())
}

/// Tous les samples de la base, triés par chemin.
pub fn lister(base: &Connection) -> Result<Vec<InfoSample>, Box<dyn Error>> {
    let mut requete = base.prepare(
        "SELECT chemin, duree_secondes, frequence, canaux, bits
         FROM fichiers
         ORDER BY chemin",
    )?;

    let lignes = requete.query_map([], |ligne| {
        let chemin: String = ligne.get(0)?;
        Ok(InfoSample {
            chemin: PathBuf::from(chemin),
            duree_secondes: ligne.get(1)?,
            frequence: ligne.get(2)?,
            canaux: ligne.get(3)?,
            bits: ligne.get(4)?,
        })
    })?;

    let mut samples = Vec::new();
    for sample in lignes {
        samples.push(sample?);
    }
    Ok(samples)
}
