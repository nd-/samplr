// samplr — étape 1 : scanner un dossier de samples et en écouter un.
//
// Utilisation :
//   cargo run -- scan ~/Samples        → liste les samples avec leurs infos
//   cargo run -- play ~/Samples/kick.wav → joue un fichier

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
    canaux: Option<usize>,
    bits: Option<u32>,
}

fn main() {
    // On récupère les arguments de la ligne de commande dans un vecteur (Vec).
    let args: Vec<String> = env::args().collect();

    // `match` compare une valeur à plusieurs motifs, un peu comme un switch.
    let resultat = match args.get(1).map(|s| s.as_str()) {
        Some("scan") if args.len() == 3 => scanner(Path::new(&args[2])),
        Some("play") if args.len() == 3 => jouer(Path::new(&args[2])),
        _ => {
            eprintln!("Utilisation :");
            eprintln!("  samplr scan <dossier>");
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
    chemin
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| EXTENSIONS_AUDIO.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Parcourt le dossier (et ses sous-dossiers) et affiche un tableau des samples.
fn scanner(dossier: &Path) -> Result<(), Box<dyn Error>> {
    if !dossier.is_dir() {
        return Err(format!("« {} » n'est pas un dossier", dossier.display()).into());
    }

    let mut nb_samples = 0;
    let mut nb_illisibles = 0;

    println!(
        "{:>8}  {:>7}  {:>4}  {:>5}  FICHIER",
        "DURÉE", "FRÉQ.", "BITS", "CAN."
    );

    for entree in WalkDir::new(dossier).sort_by_file_name() {
        // Un dossier illisible (droits insuffisants…) ne doit pas tout arrêter.
        let Ok(entree) = entree else { continue };
        let chemin = entree.path();

        if !entree.file_type().is_file() || !est_audio(chemin) {
            continue;
        }

        match lire_infos(chemin) {
            Ok(info) => {
                nb_samples += 1;
                // Chemin affiché relativement au dossier scanné, plus lisible.
                let relatif = info.chemin.strip_prefix(dossier).unwrap_or(&info.chemin);
                println!(
                    "{:>8}  {:>7}  {:>4}  {:>5}  {}",
                    formater_duree(info.duree_secondes),
                    info.frequence.map(|f| format!("{f}")).unwrap_or("?".into()),
                    info.bits.map(|b| format!("{b}")).unwrap_or("-".into()),
                    info.canaux.map(|c| format!("{c}")).unwrap_or("?".into()),
                    relatif.display()
                );
            }
            Err(_) => nb_illisibles += 1,
        }
    }

    println!("\n{nb_samples} sample(s) trouvé(s), {nb_illisibles} illisible(s).");
    Ok(())
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
        canaux: params.channels.map(|c| c.count()),
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
