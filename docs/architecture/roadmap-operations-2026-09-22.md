# Roadmap: configurabilità, code e operatività — 22 settembre 2026

Incremento implementato dopo la prima base transazionale. Priorità confermata:
ingestione IoT ad alto volume, storico e notifiche. Il codice è stato modificato e
verificato; l'istanza `athena-broker` esistente e il suo database non sono stati
aggiornati. Non è una certificazione di conformità completa o di superiorità ad
altri broker.

## Risultato

1. **Configurazione effettiva**: TOML tipizzato, override tramite ambiente,
   validazione prima del collegamento al database e `--check-config` senza stampa
   di credenziali. Configurabili binding, pool, timeout SQL/lock/HTTP, lease,
   concorrenza, polling, batch, tentativi, limiti HTTP, cache JSON-LD e conservazione.
   Chiavi sconosciute, valori malformati e combinazioni incoerenti vengono rifiutati.
   La policy di uscita viene passata esplicitamente ai componenti, senza letture
   sparse delle variabili d'ambiente durante le richieste.
2. **Ciclo di vita delle sottoscrizioni**: PATCH atomico sotto lock di riga per i
   campi supportati; nessuna perdita di aggiornamenti tra PATCH concorrenti;
   contatori gestiti dal server preservati. `isActive` controlla pausa/ripresa,
   le scadenze sono applicate anche in lettura, il contesto viene conservato.
   Supportati selettori basati sui soli watchedAttributes, throttling frazionario,
   rimozione di campi opzionali tramite NGSI-LD null e receiverInfo in formato
   KeyValuePair. Opzioni non supportate producono un errore esplicito.
3. **Consegna recuperabile**: la pausa conserva i job senza consumare tentativi;
   una sottoscrizione in pausa non impedisce ad altre di usare lo stesso endpoint.
   La verifica dello stato e del throttling avviene anche al momento della consegna.
   Ordinamento per endpoint e sottoscrizione, anche dopo un cambio di endpoint.
   I job malformati raggiungono una dead letter dopo un numero finito di tentativi.
   Deadline dell'elaborazione entro la lease, fence anche per le statistiche di
   fallimento e drain dei worker all'arresto.
4. **Costo del matching e del claim**: indice in memoria per tipo e preparazione
   dei selettori/AST una volta per lotto. Le sottoscrizioni vengono rilette a ogni
   lotto, evitando una cache persistente obsoleta tra repliche. Nuovi indici
   PostgreSQL e predicati separati evitano scansioni costose della coda per ogni
   consegna, conservando i vincoli di ordinamento.
5. **Conservazione a lotti**: eliminazione limitata e concorrente dei job completati
   e degli eventi elaborati senza riferimenti. Scadenze distinte per successi,
   fallimenti e storico; nessuna eliminazione automatica di dead letter o storico
   nella configurazione predefinita. Lo storico usa un timestamp di registrazione
   del database indipendente dal tempo di osservazione del dispositivo.
6. **Avvio/arresto e osservabilità**: readiness con verifica dello schema, del DB e
   dei task; metrica dei worker; deadline condivisa per lo spegnimento del processo.
   I controlli sulle sottoscrizioni restano utilizzabili quando l'ingestione è
   satura. Compose concede 40 secondi prima della terminazione forzata.

Configurazione completa: [default.toml](../../config/default.toml).
Istruzioni operative e limiti: [runbook](../operations.md).

## Verifiche eseguite

| Verifica | Esito |
| --- | --- |
| `cargo fmt --all --check` | PASS |
| `cargo test --offline --locked --workspace` | PASS: 30 test ordinari; cinque test espliciti ignorati per default |
| `cargo clippy --offline --locked --workspace --all-targets` | PASS, con warning stilistici preesistenti |
| `storage_integration --ignored` | PASS sul database aggiornato e su quello vuoto |
| `operations_integration --ignored` | PASS sul database aggiornato e su quello vuoto |
| `subscription_benchmark --release --ignored` | PASS, conteggio dei match verificato |
| `http_iot_benchmark --release --ignored` | PASS dopo correzione del claim; 1.000 eventi/storici e 5.000 consegne verificate |

Le due suite di integrazione utilizzano implementazioni reali, PostgreSQL/PostGIS,
router Axum e ricevitori HTTP locali. La nuova suite verifica PATCH concorrenti,
rollback di input invalidi, campi di sistema, espansione dei filtri API, scadenza e
rinnovo, pausa/ripresa, concorrenza dei worker, drain durante una richiesta HTTP,
job malformati, conservazione selettiva e timeout applicati alle connessioni.

Verificato anche l'eseguibile reale: file TOML, `--check-config`, `--healthcheck`,
readiness HTTP e uscita con SIGTERM. Le migrazioni 1–9 sono state applicate a un
secondo database inizialmente vuoto sulla porta 55433; il container temporaneo è
stato poi rimosso. Il database di test preesistente resta sulla porta 55432.
La CI è stata aggiornata per includere la suite operativa, ma non eseguita su un
runner remoto. Rimane il warning di compatibilità futura di `sqlx-postgres 0.7.4`.

## Misurazioni e problema risolto

Ambiente: macOS ARM64; binario Rust release nativo; PostgreSQL 16/PostGIS 3.4
`linux/amd64` eseguito in Docker con emulazione. Database di test con fixture già
presenti; niente confronto con hardware o broker di produzione. I ricevitori
rispondono localmente con HTTP 204 e non simulano l'elaborazione di sistemi esterni.

Il primo tentativo end-to-end ha superato il limite di 60 secondi: tutti i 1.000
eventi erano elaborati e tutti i job presenti, ma solo **366 delle 5.000 notifiche**
erano consegnate. L'analisi del piano PostgreSQL ha evidenziato il costo del controllo
correlato di ordinamento e l'assenza di un indice completo per endpoint/evento/id.
La migrazione 9 aggiunge gli indici coerenti con i filtri e l'ordinamento; il claim
usa controlli separati per endpoint e sottoscrizione. Il medesimo carico ha poi
completato tutte le consegne.

| HTTP + storico + fan-out | Misura osservata |
| --- | --- |
| Entità / mutazioni | 500 / 1.000: creazione e successivo upsert |
| Batch / richieste HTTP concorrenti | 50 entità / 4 |
| Worker / ricevitori distinti | 4 / 5 |
| Storico / notifiche | 1.000 istanze / 5.000 consegne, nessun ID duplicato osservato |
| Ingestione HTTP del burst | 2.484,92 mutazioni/s |
| Latenza richieste batch p50 / p95 | 47,48 / 175,09 ms |
| Tempo complessivo fino all'ultima consegna registrata | **6,537 s** |
| Latenza notifica p50 / p95 | 3.398,995 / 5.863,79 ms |

La latenza notifica parte dal timestamp generato dal client prima della richiesta
batch e termina all'arrivo al ricevitore. Il tempo complessivo include il drain fino
alla registrazione di consegna nel DB. Il throughput di ingestione è quello del
**burst con coda asincrona**, non il throughput sostenibile a cinque notifiche per
mutazione. L'intera catena in questa prova equivale a circa 153 mutazioni/s e
765 notifiche/s includendo il drain: non confondere questi valori con le scritture
accettate inizialmente. Nessuna garanzia di latenza produttiva deriva da questo test.

Il benchmark separato del matching usa 1.000 sottoscrizioni su 100 tipi, 10.000 eventi
e 10 candidati per evento: 100.000 match corretti, circa 515.389 eventi/s nella sola
fase di lookup/valutazione e 61,87 ms per costruire l'indice. Esclude database, HTTP,
JSON-LD e costruzione dell'indice dal throughput; non misura la capacità del broker.

Dati in forma strutturata: [benchmark JSON](roadmap-benchmark-2026-09-22.json).

## Decisioni e compatibilità

- Le migrazioni 7–9 sono additive/evolutive e non modificano i checksum 1–6.
  Possono prendere lock e riscrivere righe: la versione 7 cambia il tipo SQL del
  throttling e aggiunge recorded_at allo storico. Per aggiornare un'installazione
  esistente servono una migrazione provata su staging e lo stop delle vecchie repliche;
  non è compatibile con un rolling upgrade degli eseguibili precedenti.
- I job persistenti sono snapshot di ID, payload, endpoint e header. Un PATCH cambia
  i job futuri; quelli già materializzati mantengono lo snapshot per retry stabili.
  Stato di attività e throttling vengono invece consultati prima di ogni invio.
- La consegna rimane almeno una volta: un crash tra ricezione HTTP e commit può
  generare duplicati. Pausa/cancellazione non possono richiamare un HTTP già inviato.
- La conservazione predefinita pulisce al massimo 1.000 righe per categoria al minuto.
  È prudente per lo sviluppo; va dimensionata rispetto al tasso di scadenza misurato.
  Non sostituisce partizionamento, autovacuum o pianificazione dello spazio disco.
- Non sono stati modificati runtime, credenziali o dati dell'istanza esistente.

Per le regole di aggiornamento, attività e campi di sistema è stata consultata la
[definizione ufficiale ETSI delle operazioni NGSI-LD, clausole 5.2.12, 5.5.8 e 5.8](https://cim.etsi.org/NGSI-LD/official/clause-5.html).
Il riferimento normativo di progetto rimane ETSI GS CIM 009 V1.9.1.

## Prossime priorità ancora aperte

1. Test di lunga durata e fault injection con riavvio di processi/DB, failover e
   restore verificato; sizing su CPU nativa e carico rappresentativo.
2. Completamento delle notifiche iniziali, timeInterval, notificationTrigger,
   jsonldContext/formati avanzati e MQTT; suite ufficiale di conformance indipendente.
3. Retention/partizionamento dello storico a volume elevato, telemetria a basso costo
   e gestione autenticata delle dead letter.
4. Isolamento tenant, autenticazione/autorizzazione, restante semantica NGSI-LD di
   update/query e federazione completa. Il solo rafforzamento operativo non rende
   queste funzionalità implementate.

Stato complessivo: [IMPLEMENTATION.md](../../IMPLEMENTATION.md).
