# samplr — contexte pour Claude

## Le projet
Gestionnaire de samples audio de bureau pour **macOS et Linux** : écouter, tagger et ranger ses samples (usage musicien, à côté d'un DAW).

## Qui développe
- **Débutant complet en Rust** : expliquer chaque nouvelle notion (ownership, emprunts, `Result`, traits, lifetimes, threads…) au moment où elle apparaît dans le code, avec des mots simples. Pas de cours théorique en amont.
- Machines : un Mac M4 (Apple Silicon) et un Linux. Développement principal sous **Linux avec RustRover**.
- Habitué aux raccourcis clavier Linux. S'il faut citer des raccourcis RustRover, donner ceux du keymap Linux (ou les deux versions).

## Façon de travailler
- Avancer par **petites étapes qui compilent et tournent** : chaque étape doit pouvoir se tester avec `cargo run`.
- Écrire le code **avec des commentaires en français**. Noms de fonctions et de variables en français, comme dans l'existant.
- Garder un code simple et lisible plutôt qu'idiomatique-avancé. Pas d'abstraction prématurée.
- Quand une erreur du compilateur est collée, l'expliquer avant de la corriger.
- Avant de rendre la main : `cargo build`, `cargo clippy` et `cargo fmt` sans avertissement.
- Penser Mac **et** Linux à chaque choix (chemins, audio, glisser-déposer, empaquetage).

## Stack actuelle
- Rust édition 2024, binaire unique `samplr` (ligne de commande pour l'instant).
- `symphonia` 0.5 (feature `all`) : lecture des infos et décodage. **Garder la 0.5**, c'est la version utilisée par `rodio` ; passer à la 0.6 ferait compiler deux versions.
- `rodio` 0.23 (feature `symphonia-all`) : lecture audio. API : `DeviceSinkBuilder::open_default_sink()`, `Player::connect_new(mixer)`, `Decoder::try_from(file)`.
- `walkdir` : parcours récursif des dossiers.
- Pas encore de `clap` : les arguments sont lus à la main dans `main`.

Dépendances système Linux : `build-essential pkg-config libasound2-dev` (ALSA).

Sur le Mac, le projet est ouvert depuis le partage SMB de la machine Linux (`/Volumes/nico/Sites/projets/sample browser`). Le partage ne gère pas le verrouillage de fichiers dont Cargo a besoin (« could not create session directory lock file ») : `~/.cargo/config.toml` du Mac envoie donc la compilation dans `/Users/nico/cargo-target` (`build.target-dir`), ce qui évite aussi de mélanger binaires macOS et Linux dans `target/`. Ne pas mettre ce réglage dans le `.cargo/config.toml` du projet : le chemin n'existe pas sous Linux.

## État actuel (étape 2 terminée)
`src/main.rs` contient :
- `samplr scan <dossier>` : liste les fichiers audio (wav, aif, aiff, flac, mp3, ogg, m4a) avec durée, fréquence, bits et canaux. Les fichiers illisibles sont comptés, sans planter.
- `samplr play <fichier>` : joue un fichier sur la sortie par défaut.

Le scan et la lecture ont été testés sous Linux et sur le Mac (fichiers lus depuis le partage SMB).

Pièges audio rencontrés :
- `sleep_until_end()` revient avant que le tampon de la carte son soit vidé : sans la pause de 300 ms qui suit dans `jouer`, un sample très court (kick de 75 ms) est jeté sans être joué.
- Une sortie HDMI mise en veille par PipeWire met une à deux secondes à se réveiller et avale un sample court. Ce n'est pas un bug de samplr : le régler côté système, avec une règle WirePlumber `session.suspend-timeout-seconds = 0` sur la sortie concernée.
- Mac, fichier sur le partage SMB : un « audio stream error: A buffer underrun or overrun occurred » est apparu une fois (non reproduit en 6 essais). Cause probable : `Decoder::try_from(fichier)` lit le fichier morceau par morceau pendant la lecture, donc à travers le réseau. Cas anecdotique (partage utilisé pour les tests seulement) : on garde la lecture au fil de l'eau. Charger le fichier en mémoire retarderait le démarrage des longs fichiers, ce qui gênerait la préécoute rapide d'un sample à l'autre.
- Mac, sortie HDMI (écran DELL U2415) : macOS n'a pas de volume pour une sortie HDMI, le son part à plein niveau. SoundSource ajoute un volume logiciel, mais il l'applique **par appli** : un programme lancé en ligne de commande (`samplr`, `afplay`) est rattaché au terminal qui l'a lancé. Lancé depuis iTerm2, `samplr` apparaît sous « iTerm2 » dans SoundSource et respecte le volume : ce n'est pas un problème de `cpal`/rodio. Le son fort venait de lancements depuis un autre terminal, dont l'entrée SoundSource n'était pas atténuée. Un volume propre à samplr (`Player::set_volume`) reste utile pour l'interface.
- Les sonneries Apple `.m4r` (AAC dans du MP4, ex. `/System/Library/PrivateFrameworks/ToneLibrary.framework/Versions/A/Resources/Ringtones/Opening.m4r`) ne sont pas décodées par symphonia 0.5 : durée trouvée, canaux inconnus, aucun échantillon. Le même son réencodé par `afconvert` (AAC ou WAV) se lit bien : l'AAC est donc pris en charge, c'est la structure de ces fichiers qui pose problème.
- Quand le décodeur ne produit aucun échantillon, `play` le signale (« aucun son décodé ») : `jouer` le vérifie sur un décodeur jetable avant d'ouvrir la sortie audio.

## Feuille de route
1. ✅ Scanner un dossier et lister les samples.
2. ✅ Préécoute : `play` testé sous Linux et sur le Mac (reste : volume).
3. Base **SQLite** (`rusqlite`) : tables fichiers, tags et liaisons. Les tags restent dans la base, jamais dans les fichiers audio.
4. Commandes CLI pour tagger et rechercher (introduire `clap` à ce moment-là).
5. Surveillance des dossiers (`notify` : inotify sur Linux, FSEvents sur Mac).
6. Calcul des formes d'onde et cache dans la base.
7. Interface graphique avec **Tauri** (backend Rust + interface web, `wavesurfer.js`), préécoute au clavier.
8. Glisser-déposer des samples vers le DAW (à tester tôt, l'API diffère selon le système).
9. Analyse automatique (BPM, tonalité) et collections virtuelles.
10. Empaquetage : `.app` / `.dmg` signé et notarisé sur Mac, AppImage ou `.deb` sur Linux.

## Décisions de conception
- **Ne jamais déplacer les fichiers audio sans action explicite de l'utilisateur** : les projets DAW pointent vers leurs chemins. Privilégier les collections virtuelles (tags, favoris, dossiers intelligents).
- Données de l'appli dans le dossier standard du système (`~/.local/share` sous Linux, `~/Library/Application Support` sur Mac), via la crate `dirs` le moment venu.
