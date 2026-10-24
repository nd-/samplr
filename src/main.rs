// samplr : scanner des dossiers de samples, les tagger, les retrouver et les écouter.
//
// Exemples :
//   cargo run -- scan ~/Samples              → ajoute les samples du dossier à la base
//   cargo run -- tag ~/Samples/kick.wav 808  → ajoute le tag « 808 » à un sample
//   cargo run -- cherche 808 --chemin kick   → samples tagués 808 dont le chemin contient « kick »
//   cargo run -- help                        → toutes les commandes

mod base;

use std::error::Error;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};
use notify_debouncer_full::notify::event::{ModifyKind, RenameMode};
use notify_debouncer_full::notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{DebouncedEvent, new_debouncer};
use rusqlite::Connection;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use walkdir::WalkDir;

/// Extensions de fichiers que l'on considère comme des samples.
const EXTENSIONS_AUDIO: [&str; 7] = ["wav", "aif", "aiff", "flac", "mp3", "ogg", "m4a"];

/// Les infos qu'on extrait de chaque fichier.
/// `Option<T>` veut dire « peut-être une valeur, peut-être rien » :
/// certains fichiers ne déclarent pas tout.
struct InfoSample {
    chemin: PathBuf,
    duree_secondes: Option<f64>,
    frequence: Option<u32>,
    canaux: Option<u32>,
    bits: Option<u32>,
    /// Vide tant que le sample n'a pas été relu depuis la base.
    tags: Vec<String>,
}

// clap lit cette description pour analyser la ligne de commande : chaque
// variante de `Commande` devient une sous-commande (`Scan` → `samplr scan`),
// ses champs en deviennent les arguments, et les commentaires `///`
// deviennent l'aide affichée par `samplr help`.

/// Gestionnaire de samples audio : scanner, tagger, chercher, écouter.
#[derive(Parser)]
#[command(version)]
struct Arguments {
    #[command(subcommand)]
    commande: Commande,
}

#[derive(Subcommand)]
enum Commande {
    /// Ajoute à la base les samples d'un dossier et de ses sous-dossiers
    Scan { dossier: PathBuf },
    /// Affiche tous les samples de la base, avec leurs tags
    Liste,
    /// Suit les dossiers scannés et met la base à jour en direct (Ctrl+C pour arrêter)
    Surveille,
    /// Joue un fichier sur la sortie audio par défaut
    Play { fichier: PathBuf },
    /// Ajoute des tags à un sample (l'ajoute à la base s'il n'y est pas)
    Tag {
        fichier: PathBuf,
        #[arg(required = true)]
        tags: Vec<String>,
    },
    /// Retire des tags d'un sample
    Untag {
        fichier: PathBuf,
        #[arg(required = true)]
        tags: Vec<String>,
    },
    /// Liste les tags existants et leur nombre de samples
    Tags,
    /// Cherche les samples qui portent tous les tags donnés
    Cherche {
        tags: Vec<String>,
        /// Ne garder que les samples dont le chemin contient ce texte
        #[arg(long)]
        chemin: Option<String>,
    },
}

fn main() {
    // En cas d'arguments invalides, clap affiche l'aide et arrête le programme.
    let arguments = Arguments::parse();

    // `match` compare une valeur à plusieurs motifs, un peu comme un switch.
    // Chaque motif « ouvre » la variante et nomme ses champs.
    let resultat = match arguments.commande {
        Commande::Scan { dossier } => scanner(&dossier),
        Commande::Liste => chercher(&[], None),
        Commande::Surveille => surveiller(),
        Commande::Play { fichier } => jouer(&fichier),
        Commande::Tag { fichier, tags } => taguer(&fichier, &tags),
        Commande::Untag { fichier, tags } => detaguer(&fichier, &tags),
        Commande::Tags => lister_tags(),
        Commande::Cherche { tags, chemin } => chercher(&tags, chemin.as_deref()),
    };

    // Si une fonction a renvoyé une erreur, on l'affiche proprement.
    if let Err(erreur) = resultat {
        // `samplr liste | head` : `head` ferme le tuyau dès qu'il a ses lignes,
        // et l'écriture suivante échoue (« broken pipe »). Ce n'est pas une
        // erreur de samplr : on s'arrête sans rien dire, comme `ls` ou `grep`.
        // `downcast_ref` regarde si l'erreur, de type inconnu ici, est une
        // erreur d'entrée-sortie.
        if let Some(erreur_es) = erreur.downcast_ref::<io::Error>()
            && erreur_es.kind() == io::ErrorKind::BrokenPipe
        {
            process::exit(0);
        }
        eprintln!("Erreur : {erreur}");
        process::exit(1);
    }
}

/// Vrai pour `._kick.wav` : métadonnées que macOS dépose à côté de `kick.wav`
/// sur un disque qui ne sait pas les stocker (clé USB, partage réseau). Pas du son.
fn est_metadonnees_mac(chemin: &Path) -> bool {
    chemin
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|nom| nom.starts_with("._"))
}

/// Vrai si le fichier a une extension audio connue (insensible à la casse).
fn est_audio(chemin: &Path) -> bool {
    if est_metadonnees_mac(chemin) {
        return false;
    }

    chemin
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| EXTENSIONS_AUDIO.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Enregistre dans la base le dossier et les samples qu'il contient, puis
/// les affiche sous forme de tableau.
fn scanner(dossier: &Path) -> Result<(), Box<dyn Error>> {
    if !dossier.is_dir() {
        return Err(format!("« {} » n'est pas un dossier", dossier.display()).into());
    }
    // Chemin absolu : la base doit retrouver les fichiers quel que soit le
    // dossier d'où l'on relance samplr.
    let dossier = dossier.canonicalize()?;

    let mut base = base::ouvrir()?;
    // Une transaction regroupe toutes les écritures : SQLite n'écrit sur le
    // disque qu'une fois, au `commit`, au lieu d'une fois par sample (des
    // centaines de fois plus lent sur un gros dossier).
    let transaction = base.transaction()?;
    base::ajouter_dossier(&transaction, &dossier)?;
    let (samples, nb_illisibles) = indexer(&transaction, &dossier);
    transaction.commit()?;

    // L'affichage n'est qu'un compte rendu : si la sortie est fermée
    // (`samplr scan … | head`), on ignore l'échec d'écriture (`let _ =`).
    let mut sortie = io::stdout().lock();
    let _ = afficher_entete(&mut sortie);
    for info in &samples {
        // Chemin affiché relativement au dossier scanné, plus lisible.
        let relatif = info.chemin.strip_prefix(&dossier).unwrap_or(&info.chemin);
        let _ = afficher_ligne(&mut sortie, info, relatif);
    }
    let _ = writeln!(
        sortie,
        "\n{} sample(s) enregistré(s), {nb_illisibles} illisible(s).",
        samples.len()
    );
    Ok(())
}

/// Parcourt `dossier` et ses sous-dossiers et enregistre chaque sample dans la
/// base. Retourne les samples enregistrés et le nombre de fichiers illisibles.
fn indexer(base: &Connection, dossier: &Path) -> (Vec<InfoSample>, usize) {
    let mut samples = Vec::new();
    let mut nb_illisibles = 0;

    for entree in WalkDir::new(dossier).sort_by_file_name() {
        // Un dossier illisible (droits insuffisants…) ne doit pas tout arrêter.
        let Ok(entree) = entree else { continue };
        let chemin = entree.path();

        if !entree.file_type().is_file() || !est_audio(chemin) {
            continue;
        }

        let Ok(info) = lire_infos(chemin) else {
            nb_illisibles += 1;
            continue;
        };
        if let Err(erreur) = base::enregistrer(base, &info) {
            eprintln!("{} : {erreur}", chemin.display());
            nb_illisibles += 1;
            continue;
        }
        samples.push(info);
    }
    (samples, nb_illisibles)
}

/// Rattrape les changements faits pendant que samplr ne tournait pas, puis
/// suit en direct les dossiers scannés jusqu'à Ctrl+C.
fn surveiller() -> Result<(), Box<dyn Error>> {
    let mut base = base::ouvrir()?;

    // Un dossier scanné qui est dans un autre dossier scanné est déjà suivi
    // avec lui : le surveiller deux fois doublerait chaque événement.
    let tous = base::dossiers(&base)?;
    let racines: Vec<PathBuf> = tous
        .iter()
        .filter(|d| !tous.iter().any(|autre| autre != *d && d.starts_with(autre)))
        .cloned()
        .collect();
    if racines.is_empty() {
        return Err("aucun dossier à surveiller : lance d'abord « samplr scan <dossier> »".into());
    }

    // Le surveillant tourne dans son propre thread et dépose ses événements
    // dans un canal : `envoi` reste chez lui, `reception` chez nous.
    let (envoi, reception) = mpsc::channel();
    // Les événements d'un même fichier sont regroupés pendant une seconde :
    // une copie produit des dizaines d'écritures, on ne relit le fichier
    // qu'une fois qu'elle est finie.
    let mut surveillant = new_debouncer(Duration::from_secs(1), None, envoi)?;

    let mut nb_surveilles = 0;
    for racine in &racines {
        if !racine.is_dir() {
            eprintln!(
                "{} introuvable (disque débranché ?) : pas surveillé",
                racine.display()
            );
            continue;
        }
        // On commence à surveiller avant le rattrapage : un fichier ajouté
        // pendant qu'il tourne ne passe pas entre les deux.
        surveillant.watch(racine, RecursiveMode::Recursive)?;
        rattraper(&mut base, racine)?;
        nb_surveilles += 1;
    }
    if nb_surveilles == 0 {
        return Err("aucun des dossiers scannés n'est disponible".into());
    }

    println!("Surveillance en cours. Ctrl+C pour arrêter.");
    // La boucle attend le prochain lot d'événements ; elle ne se termine
    // jamais d'elle-même, Ctrl+C arrête le programme.
    for lot in reception {
        match lot {
            Ok(evenements) => {
                for evenement in &evenements {
                    if let Err(erreur) = traiter_evenement(&base, &racines, evenement) {
                        eprintln!("Erreur : {erreur}");
                    }
                }
            }
            Err(erreurs) => {
                for erreur in erreurs {
                    eprintln!("Erreur de surveillance : {erreur}");
                }
            }
        }
    }
    Ok(())
}

/// Met la base à jour pour `racine` : retire les samples qui ont disparu du
/// disque, ajoute les nouveaux et rafraîchit les autres.
fn rattraper(base: &mut Connection, racine: &Path) -> Result<(), Box<dyn Error>> {
    let transaction = base.transaction()?;

    let mut nb_retires = 0;
    for chemin in base::fichiers_sous(&transaction, racine)? {
        if !chemin.exists() {
            nb_retires += base::oublier(&transaction, &chemin)?;
        }
    }
    let (samples, nb_illisibles) = indexer(&transaction, racine);
    transaction.commit()?;

    println!(
        "{} : {} sample(s), {nb_retires} retiré(s), {nb_illisibles} illisible(s)",
        racine.display(),
        samples.len()
    );
    Ok(())
}

/// Reporte dans la base un événement du système de fichiers.
fn traiter_evenement(
    base: &Connection,
    racines: &[PathBuf],
    evenement: &DebouncedEvent,
) -> Result<(), Box<dyn Error>> {
    match evenement.kind {
        // Un renommage dont on connaît les deux bouts : on déplace la ligne
        // existante pour garder les tags.
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            let (ancien, nouveau) = (&evenement.paths[0], &evenement.paths[1]);
            let nb_renommes = base::renommer(base, ancien, nouveau)?;
            if nb_renommes > 0 {
                println!("→ {} → {}", ancien.display(), nouveau.display());
            }
            // Le nouveau nom peut changer la donne : `kick.tmp` devenu
            // `kick.wav` ou dossier arrivé d'ailleurs (rien à renommer en
            // base), `kick.wav` devenu `kick.bak` (plus un sample).
            if nb_renommes == 0 || (nouveau.is_file() && !est_audio(nouveau)) {
                synchroniser(base, racines, nouveau, true)?;
            }
            Ok(())
        }
        // Lire un fichier ne change rien pour la base.
        EventKind::Access(_) => Ok(()),
        _ => {
            let est_arrivee = matches!(
                evenement.kind,
                EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
            );
            for chemin in &evenement.paths {
                synchroniser(base, racines, chemin, est_arrivee)?;
            }
            Ok(())
        }
    }
}

/// Aligne la base sur ce qui se trouve en ce moment à `chemin`.
/// `est_arrivee` : le chemin vient d'apparaître (création ou renommage). Un
/// dossier n'est parcouru que dans ce cas ; sinon, la moindre modification
/// dans un dossier relirait tout son contenu.
fn synchroniser(
    base: &Connection,
    racines: &[PathBuf],
    chemin: &Path,
    est_arrivee: bool,
) -> Result<(), Box<dyn Error>> {
    if !chemin.exists() {
        // Disque débranché : tout disparaît d'un coup, mais les samples
        // existent toujours. Les oublier ferait perdre leurs tags.
        let racine_presente = racines
            .iter()
            .any(|racine| chemin.starts_with(racine) && racine.is_dir());
        if !racine_presente {
            return Ok(());
        }
        let nb_oublies = base::oublier(base, chemin)?;
        if nb_oublies > 0 {
            println!("- {} ({nb_oublies} sample(s))", chemin.display());
        }
    } else if chemin.is_dir() {
        if est_arrivee {
            let (samples, _) = indexer(base, chemin);
            if !samples.is_empty() {
                println!("+ {} ({} sample(s))", chemin.display(), samples.len());
            }
        }
    } else if est_audio(chemin) {
        // Un fichier en cours de copie peut encore être illisible : la fin de
        // la copie produira un nouvel événement, et on réessaiera.
        if let Ok(info) = lire_infos(chemin) {
            base::enregistrer(base, &info)?;
            println!("+ {}", chemin.display());
        }
    } else if base::oublier(base, chemin)? > 0 {
        // Renommé en un nom qui n'est plus audio (`kick.wav` → `kick.bak`).
        println!("- {}", chemin.display());
    }
    Ok(())
}

/// Affiche les samples de la base qui portent tous les `tags` et dont le
/// chemin contient `chemin` ; sans tag ni chemin, tous les samples.
fn chercher(tags: &[String], chemin: Option<&str>) -> Result<(), Box<dyn Error>> {
    let base = base::ouvrir()?;
    let samples = base::chercher(&base, tags, chemin.unwrap_or(""))?;

    let mut sortie = io::stdout().lock();
    afficher_entete(&mut sortie)?;
    for info in &samples {
        afficher_ligne(&mut sortie, info, &info.chemin)?;
    }
    writeln!(sortie, "\n{} sample(s) trouvé(s).", samples.len())?;
    Ok(())
}

/// Ajoute des tags à un sample. Le fichier est relu à chaque fois : s'il
/// n'était pas encore dans la base, il y entre ; sinon ses infos sont rafraîchies.
fn taguer(fichier: &Path, tags: &[String]) -> Result<(), Box<dyn Error>> {
    let fichier = fichier.canonicalize()?;
    if !est_audio(&fichier) {
        return Err(format!("{} n'est pas un fichier audio reconnu", fichier.display()).into());
    }
    let info = lire_infos(&fichier)?;

    let base = base::ouvrir()?;
    let fichier_id = base::enregistrer(&base, &info)?;
    let nb_ajoutes = base::ajouter_tags(&base, fichier_id, tags)?;

    println!("{nb_ajoutes} tag(s) ajouté(s) à {}", fichier.display());
    Ok(())
}

fn detaguer(fichier: &Path, tags: &[String]) -> Result<(), Box<dyn Error>> {
    let fichier = fichier.canonicalize()?;

    let base = base::ouvrir()?;
    let fichier_id = base::id_fichier(&base, &fichier)?
        .ok_or_else(|| format!("{} n'est pas dans la base", fichier.display()))?;
    let nb_retires = base::retirer_tags(&base, fichier_id, tags)?;

    println!("{nb_retires} tag(s) retiré(s) de {}", fichier.display());
    Ok(())
}

fn lister_tags() -> Result<(), Box<dyn Error>> {
    let base = base::ouvrir()?;
    let tags = base::lister_tags(&base)?;

    let mut sortie = io::stdout().lock();
    for (nom, nb_samples) in &tags {
        writeln!(sortie, "{nb_samples:>5}  {nom}")?;
    }
    writeln!(sortie, "\n{} tag(s).", tags.len())?;
    Ok(())
}

// Les fonctions d'affichage écrivent avec `writeln!` dans la `sortie` reçue
// plutôt qu'avec `println!` : `println!` arrête le programme sur une erreur
// d'écriture, `writeln!` la retourne et laisse l'appelant décider.

fn afficher_entete(sortie: &mut impl Write) -> io::Result<()> {
    writeln!(
        sortie,
        "{:>8}  {:>7}  {:>4}  {:>5}  FICHIER",
        "DURÉE", "FRÉQ.", "BITS", "CAN."
    )
}

/// Une ligne du tableau ; `chemin` est le chemin à afficher, complet ou raccourci.
/// Les tags éventuels suivent le chemin, entre crochets.
fn afficher_ligne(sortie: &mut impl Write, info: &InfoSample, chemin: &Path) -> io::Result<()> {
    let tags = if info.tags.is_empty() {
        String::new()
    } else {
        format!("  [{}]", info.tags.join(" "))
    };
    writeln!(
        sortie,
        "{:>8}  {:>7}  {:>4}  {:>5}  {}{tags}",
        formater_duree(info.duree_secondes),
        info.frequence.map(|f| format!("{f}")).unwrap_or("?".into()),
        info.bits.map(|b| format!("{b}")).unwrap_or("-".into()),
        info.canaux.map(|c| format!("{c}")).unwrap_or("?".into()),
        chemin.display()
    )
}

/// Ouvre un fichier audio avec symphonia et lit ses caractéristiques
/// sans décoder tout le son (c'est rapide).
fn lire_infos(chemin: &Path) -> Result<InfoSample, Box<dyn Error>> {
    let fichier = File::open(chemin)?; // le `?` renvoie l'erreur à l'appelant s'il y en a une
    let flux = MediaSourceStream::new(Box::new(fichier), Default::default());

    // On donne l'extension en indice pour aider symphonia à reconnaître le format.
    let mut indice = Hint::new();
    if let Some(ext) = chemin.extension().and_then(|e| e.to_str()) {
        indice.with_extension(ext);
    }

    let sonde = symphonia::default::get_probe().format(
        &indice,
        flux,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;

    let piste = sonde
        .format
        .default_track()
        .ok_or("aucune piste audio dans le fichier")?;
    let params = &piste.codec_params;

    // Durée = nombre d'échantillons / fréquence d'échantillonnage.
    let duree_secondes = match (params.n_frames, params.sample_rate) {
        (Some(frames), Some(freq)) => Some(frames as f64 / freq as f64),
        _ => None,
    };

    Ok(InfoSample {
        chemin: chemin.to_path_buf(),
        duree_secondes,
        frequence: params.sample_rate,
        canaux: params.channels.map(|c| c.count() as u32),
        bits: params.bits_per_sample,
        tags: Vec::new(),
    })
}

/// 1.234 → "1.23 s", 75.0 → "1:15"
fn formater_duree(duree: Option<f64>) -> String {
    match duree {
        Some(s) if s < 60.0 => format!("{s:.2} s"),
        Some(s) => format!("{}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64),
        None => "?".to_string(),
    }
}

/// Joue un fichier sur la sortie audio par défaut et attend la fin.
fn jouer(chemin: &Path) -> Result<(), Box<dyn Error>> {
    if est_metadonnees_mac(chemin) {
        return Err(format!(
            "{} : métadonnées macOS, pas un fichier audio",
            chemin.display()
        )
        .into());
    }

    // Certains fichiers s'ouvrent sans erreur mais ne produisent aucun échantillon
    // (sonneries `.m4r` d'Apple). On le vérifie sur un décodeur jetable : lire
    // le premier échantillon sur celui qui joue le consommerait, et décalerait
    // les canaux gauche et droit d'un fichier stéréo.
    let mut sonde = rodio::Decoder::try_from(File::open(chemin)?)?;
    if sonde.next().is_none() {
        return Err("aucun son décodé (format reconnu, mais contenu illisible)".into());
    }

    // Ouvre la sortie audio par défaut (CoreAudio sur Mac, ALSA sur Linux).
    let mut sortie = rodio::DeviceSinkBuilder::open_default_sink()?;
    sortie.log_on_drop(false); // évite un message parasite à la fermeture

    let lecteur = rodio::Player::connect_new(sortie.mixer());
    let fichier = File::open(chemin)?;
    lecteur.append(rodio::Decoder::try_from(fichier)?);

    println!("▶ Lecture de {}", chemin.display());
    lecteur.sleep_until_end();
    // `sleep_until_end` revient quand rodio a fourni tous les échantillons,
    // pas quand ils sont sortis des enceintes : il en reste dans le tampon de
    // la carte son. Fermer la sortie tout de suite les jetterait, et un sample
    // très court (un kick de 75 ms) disparaîtrait en entier.
    thread::sleep(Duration::from_millis(300));
    Ok(())
}
