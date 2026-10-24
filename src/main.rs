// samplr : scanner un dossier de samples, les garder dans une base et en écouter un.
//
// Utilisation :
//   cargo run -- scan ~/Samples          → ajoute les samples du dossier à la base
//   cargo run -- liste                   → affiche les samples de la base
//   cargo run -- play ~/Samples/kick.wav → joue un fichier

mod base;

use std::env;
use std::error::Error;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process;
use std::thread;
use std::time::Duration;

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
}

fn main() {
    // On récupère les arguments de la ligne de commande dans un vecteur (Vec).
    let args: Vec<String> = env::args().collect();

    // `match` compare une valeur à plusieurs motifs, un peu comme un switch.
    let resultat = match args.get(1).map(|s| s.as_str()) {
        Some("scan") if args.len() == 3 => scanner(Path::new(&args[2])),
        Some("liste") if args.len() == 2 => lister(),
        Some("play") if args.len() == 3 => jouer(Path::new(&args[2])),
        _ => {
            eprintln!("Utilisation :");
            eprintln!("  samplr scan <dossier>");
            eprintln!("  samplr liste");
            eprintln!("  samplr play <fichier>");
            process::exit(1);
        }
    };

    // Si une fonction a renvoyé une erreur, on l'affiche proprement.
    if let Err(erreur) = resultat {
        eprintln!("Erreur : {erreur}");
        process::exit(1);
    }
}

/// Vrai si le fichier a une extension audio connue (insensible à la casse).
fn est_audio(chemin: &Path) -> bool {
    // `._kick.wav` : métadonnées que macOS dépose à côté de `kick.wav` sur un
    // disque qui ne sait pas les stocker (clé USB, partage réseau). Pas du son.
    let nom = chemin.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if nom.starts_with("._") {
        return false;
    }

    chemin
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| EXTENSIONS_AUDIO.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Parcourt le dossier (et ses sous-dossiers), affiche un tableau des samples
/// et les enregistre dans la base.
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

    let mut nb_samples = 0;
    let mut nb_illisibles = 0;

    afficher_entete();

    for entree in WalkDir::new(&dossier).sort_by_file_name() {
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
        if let Err(erreur) = base::enregistrer(&transaction, &info) {
            eprintln!("{} : {erreur}", chemin.display());
            nb_illisibles += 1;
            continue;
        }

        nb_samples += 1;
        // Chemin affiché relativement au dossier scanné, plus lisible.
        let relatif = info.chemin.strip_prefix(&dossier).unwrap_or(&info.chemin);
        afficher_ligne(&info, relatif);
    }

    transaction.commit()?;
    println!("\n{nb_samples} sample(s) enregistré(s), {nb_illisibles} illisible(s).");
    Ok(())
}

/// Affiche tous les samples enregistrés dans la base.
fn lister() -> Result<(), Box<dyn Error>> {
    let base = base::ouvrir()?;
    let samples = base::lister(&base)?;

    afficher_entete();
    for info in &samples {
        afficher_ligne(info, &info.chemin);
    }
    println!("\n{} sample(s) dans la base.", samples.len());
    Ok(())
}

fn afficher_entete() {
    println!(
        "{:>8}  {:>7}  {:>4}  {:>5}  FICHIER",
        "DURÉE", "FRÉQ.", "BITS", "CAN."
    );
}

/// Une ligne du tableau ; `chemin` est le chemin à afficher, complet ou raccourci.
fn afficher_ligne(info: &InfoSample, chemin: &Path) {
    println!(
        "{:>8}  {:>7}  {:>4}  {:>5}  {}",
        formater_duree(info.duree_secondes),
        info.frequence.map(|f| format!("{f}")).unwrap_or("?".into()),
        info.bits.map(|b| format!("{b}")).unwrap_or("-".into()),
        info.canaux.map(|c| format!("{c}")).unwrap_or("?".into()),
        chemin.display()
    );
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
