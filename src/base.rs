// Base SQLite de samplr : la liste des samples connus et leurs tags.
// Les tags vivent ici, jamais dans les fichiers audio.

use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

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
/// Retourne l'`id` du sample.
pub fn enregistrer(base: &Connection, info: &InfoSample) -> Result<i64, Box<dyn Error>> {
    let chemin = chemin_en_texte(&info.chemin)?;

    // `ON CONFLICT … DO UPDATE` met à jour la ligne existante au lieu de la
    // remplacer : le sample garde son `id`, donc ses tags. `RETURNING id`
    // donne cet `id` dans les deux cas, ajout ou mise à jour.
    let id = base.query_row(
        "INSERT INTO fichiers (chemin, duree_secondes, frequence, canaux, bits)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (chemin) DO UPDATE SET
             duree_secondes = excluded.duree_secondes,
             frequence      = excluded.frequence,
             canaux         = excluded.canaux,
             bits           = excluded.bits
         RETURNING id",
        params![
            chemin,
            info.duree_secondes,
            info.frequence,
            info.canaux,
            info.bits
        ],
        |ligne| ligne.get(0),
    )?;
    Ok(id)
}

/// L'`id` d'un sample d'après son chemin, ou `None` s'il n'est pas dans la base.
pub fn id_fichier(base: &Connection, chemin: &Path) -> Result<Option<i64>, Box<dyn Error>> {
    let chemin = chemin_en_texte(chemin)?;
    // `optional()` transforme « aucune ligne trouvée », qui serait une erreur
    // pour `query_row`, en un simple `None`.
    let id = base
        .query_row(
            "SELECT id FROM fichiers WHERE chemin = ?1",
            [chemin],
            |ligne| ligne.get(0),
        )
        .optional()?;
    Ok(id)
}

/// Ajoute des tags à un sample. Retourne le nombre de tags réellement ajoutés
/// (un tag que le sample porte déjà ne compte pas).
pub fn ajouter_tags(
    base: &Connection,
    fichier_id: i64,
    tags: &[String],
) -> Result<usize, Box<dyn Error>> {
    let mut nb_ajoutes = 0;
    for tag in tags {
        let tag = normaliser_tag(tag)?;
        base.execute(
            "INSERT INTO tags (nom) VALUES (?1) ON CONFLICT (nom) DO NOTHING",
            [&tag],
        )?;
        // `execute` retourne le nombre de lignes touchées : 0 si la liaison
        // existait déjà (`OR IGNORE`).
        nb_ajoutes += base.execute(
            "INSERT OR IGNORE INTO fichiers_tags (fichier_id, tag_id)
             SELECT ?1, id FROM tags WHERE nom = ?2",
            params![fichier_id, tag],
        )?;
    }
    Ok(nb_ajoutes)
}

/// Retire des tags d'un sample. Retourne le nombre de tags réellement retirés.
pub fn retirer_tags(
    base: &Connection,
    fichier_id: i64,
    tags: &[String],
) -> Result<usize, Box<dyn Error>> {
    let mut nb_retires = 0;
    for tag in tags {
        let tag = normaliser_tag(tag)?;
        nb_retires += base.execute(
            "DELETE FROM fichiers_tags
             WHERE fichier_id = ?1
               AND tag_id = (SELECT id FROM tags WHERE nom = ?2)",
            params![fichier_id, tag],
        )?;
    }
    // Un tag qui ne sert plus à aucun sample disparaît de la liste des tags.
    base.execute(
        "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM fichiers_tags)",
        [],
    )?;
    Ok(nb_retires)
}

/// Tous les tags, avec le nombre de samples qui portent chacun, par ordre alphabétique.
pub fn lister_tags(base: &Connection) -> Result<Vec<(String, u32)>, Box<dyn Error>> {
    let mut requete = base.prepare(
        "SELECT t.nom, COUNT(*)
         FROM tags t JOIN fichiers_tags ft ON ft.tag_id = t.id
         GROUP BY t.id
         ORDER BY t.nom",
    )?;
    let lignes = requete.query_map([], |ligne| Ok((ligne.get(0)?, ligne.get(1)?)))?;

    let mut tags = Vec::new();
    for tag in lignes {
        tags.push(tag?);
    }
    Ok(tags)
}

/// Les samples qui portent **tous** les `tags` donnés et dont le chemin contient
/// `texte` (sans tenir compte des majuscules), triés par chemin. Sans tag et
/// avec un texte vide, tous les samples de la base.
pub fn chercher(
    base: &Connection,
    tags: &[String],
    texte: &str,
) -> Result<Vec<InfoSample>, Box<dyn Error>> {
    // `instr` cherche le texte tel quel : avec `LIKE`, un `_` dans le texte
    // (fréquent dans les noms de samples) servirait de joker.
    let mut sql = String::from(
        "SELECT f.chemin, f.duree_secondes, f.frequence, f.canaux, f.bits,
                (SELECT group_concat(t.nom, ' ' ORDER BY t.nom)
                 FROM fichiers_tags ft JOIN tags t ON t.id = ft.tag_id
                 WHERE ft.fichier_id = f.id)
         FROM fichiers f
         WHERE instr(lower(f.chemin), lower(?1)) > 0",
    );
    let mut valeurs = vec![texte.to_string()];

    // Le nombre de tags varie : on ajoute une condition, et un `?N`, par tag.
    for tag in tags {
        valeurs.push(normaliser_tag(tag)?);
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM fichiers_tags ft JOIN tags t ON t.id = ft.tag_id
                          WHERE ft.fichier_id = f.id AND t.nom = ?{})",
            valeurs.len()
        ));
    }
    sql.push_str(" ORDER BY f.chemin");

    let mut requete = base.prepare(&sql)?;
    let lignes = requete.query_map(params_from_iter(&valeurs), |ligne| {
        let chemin: String = ligne.get(0)?;
        let tags: Option<String> = ligne.get(5)?;
        Ok(InfoSample {
            chemin: PathBuf::from(chemin),
            duree_secondes: ligne.get(1)?,
            frequence: ligne.get(2)?,
            canaux: ligne.get(3)?,
            bits: ligne.get(4)?,
            tags: tags
                .map(|t| t.split(' ').map(String::from).collect())
                .unwrap_or_default(),
        })
    })?;

    let mut samples = Vec::new();
    for sample in lignes {
        samples.push(sample?);
    }
    Ok(samples)
}

/// Un tag est rangé en minuscules et sans espace autour : `Kick` et ` kick`
/// désignent le même tag. Les espaces à l'intérieur sont refusés, ils servent
/// de séparateur à l'affichage.
fn normaliser_tag(tag: &str) -> Result<String, Box<dyn Error>> {
    let tag = tag.trim().to_lowercase();
    if tag.is_empty() || tag.contains(char::is_whitespace) {
        return Err(format!("tag invalide : « {tag} » (vide ou avec des espaces)").into());
    }
    Ok(tag)
}

/// Le chemin est stocké en texte : un nom de fichier qui n'est pas de
/// l'UTF-8 valide (possible sous Linux) ne pourrait pas être retrouvé.
fn chemin_en_texte(chemin: &Path) -> Result<&str, Box<dyn Error>> {
    Ok(chemin
        .to_str()
        .ok_or("nom de fichier avec des caractères invalides")?)
}
