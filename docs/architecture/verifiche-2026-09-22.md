# Athena: registro delle verifiche dell'analisi

Data: 22 settembre 2026. Workspace: `/Users/francescodibella/athena_broker`.

È stata effettuata un'analisi di sorgenti, manifest, migrazioni, container configuration, test e script di benchmark. Skill applicate: `architect-review` e `architecture`, con il relativo framework di alternative e trade-off. La skill `analyze-project` è stata letta ma esclusa perché riguarda postmortem di sessioni Antigravity; la skill generale di performance è stata letta ma non eseguita come workflow di ottimizzazione, non essendoci una baseline di profiling. Non sono stati usati subagenti.

## Ambiente e limiti

- `git status --short`: la directory non è un repository Git. Non è disponibile un commit autorevole a cui attribuire l'analisi.
- All'inizio non era presente `Cargo.lock`; la cache non conteneva `chrono`.
- Toolchain usata: `rustc 1.98.1 (48a229cea 2026-09-01)`, `cargo 1.98.1 (797e8a9bc 2026-08-05)`. Non è stato verificato l'MSRV 1.80 dichiarato dal README.
- Dopo l'autorizzazione al download delle dipendenze, Cargo ha risolto 300 package. Build artifact in `/tmp/athena-architecture-audit-target`; il lockfile generato è stato spostato in `/tmp/athena-architecture-audit-2026-09-22.Cargo.lock`, lasciando invariati i manifest e i sorgenti applicativi.
- Docker mostrava `athena-broker` e `athena-postgres` attivi, quest'ultimo con immagine `postgis/postgis:16-3.4`. Nessun container è stato creato, riavviato o modificato.
- Le verifiche HTTP sono richieste GET su health, metriche e selettori artificiali, senza scrittura o cancellazione di entità.
- Non è stata verificata l'identità binaria fra il container attivo e i sorgenti locali. I riscontri HTTP sono coerenti con i difetti osservati nel codice, ma la build/container va resa tracciabile in W00.
- Nessun carico prestazionale, crash test o suite ETSI ufficiale è stato eseguito. Non sono disponibili in questo audit valori dimostrati di throughput, latenze sostenute o copertura percentuale.
- I documenti prodotti hanno superato il controllo dei collegamenti locali. Il [manifest SHA-256](sorgenti-sha256-2026-09-22.json) identifica 71 file di sorgente, test, script e configurazione dello snapshot analizzato; non sostituisce un commit Git o la provenienza dell'immagine container.

## Compilazione e test eseguiti

| Comando | Risultato | Interpretazione |
|---|---|---|
| `cargo test --workspace --offline` | Exit 101: `chrono` non presente nella cache | Limite iniziale dell'ambiente, non difetto del broker |
| `cargo test --workspace --target-dir /tmp/athena-architecture-audit-target` | Exit 101: `verification_test` non compila; 18 errori, a partire da import `thiserror` non risolti | Test radice che includono file interni senza le dipendenze necessarie |
| Ripetizione offline con `--message-format short` | Stesso errore di compilazione | Conferma che il problema rimane dopo il download delle dipendenze |
| `cargo test --workspace --lib --offline --target-dir /tmp/athena-architecture-audit-target --quiet` | JSON-LD 2/2; model 4/4; query 6/7; API 0 test. Interruzione per il test query fallito | Non è una suite completa verde |
| `cargo test -p athena-subscription -p athena-storage --lib --offline --target-dir /tmp/athena-architecture-audit-target --quiet` | Subscription 1/1; storage 0 test | Completa la verifica delle librerie rimanenti |

Totale delle unit test dei crate eseguite: **13 passate, 1 fallita**. Il fallimento è `athena-query::tests::test_sql_compilation`: cambia la disposizione delle parentesi nella stringa SQL. Da questo test non si deduce che la query sia semanticamente errata. Occorrono confronti dei risultati su database reale.

`tests/standalone_runner.rs` contiene copie autonome di parser, compiler e mock; `tests/bench_runner.rs` include quel file. I loro risultati non sostituiscono la verifica delle librerie effettivamente usate dal broker.

## Riscontri HTTP

Base URL: `http://localhost:8080`. Le richieste temporali e collection sotto usavano `Accept: application/ld+json`. I riscontri sono stati ottenuti il 22 settembre 2026 circa alle 09:00–09:01 UTC.

| Richiesta | Risposta osservata |
|---|---|
| `GET /health` | 200; JSON con `status=healthy` e `version=0.1.0` |
| `GET /ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Audit:Nonexistent?timeproperty=createdAt&timerel=after&timeAt=2026-01-01T00%3A00%3A00Z` | 500; dettaglio DB: `column "created_at" does not exist` |
| Stesso ID con `timeproperty=modifiedAt&lastN=1&timerel=after&timeAt=2026-01-01T00%3A00%3A00Z` | 500; dettaglio DB: `column "modified_at" does not exist` |
| `GET /ngsi-ld/v1/entities?type=urn:ngsi-ld:Audit:Nonexistent` | 200, body `[]`, `Content-Type: application/json` malgrado Accept richiedesse solo `application/ld+json` |
| `GET /metrics` | Solo HELP/TYPE e `athena_broker_up 1`; nessun contatore di traffico o istogramma di latenza |

Le due risposte 500 non dipendono dalla presenza dell'entità di prova: falliscono per la struttura della query SQL. Il nome `Nonexistent` è un selettore artificiale; nessuna entità di test è stata creata.

## Evidenze statiche e verifiche ancora da eseguire

Gli altri rilievi E03–E20 nel [piano](piano-evoluzione-2026-09-22.md) derivano dalla lettura del codice. Non sono stati eseguiti exploit, query con possibile panic, batch che possano modificare dati o test che interrompano il servizio attivo.

Da riprodurre sul banco di prova isolato: rollback batch dopo errore intermedio; collisione di dataset temporali; decoding COUNT; perdita di eventi al riavvio; race degli update e del throttling; SSRF/redirect/DNS; distribuzione del matching; query e paginazione federate; uso effettivo degli indici; consumo di risorse e capacità sostenibile.

## Fonti consultate e uso nel piano

- Versione stabile di riferimento e inventario di capability: [ETSI GS CIM 009 v1.9.1](https://www.etsi.org/deliver/etsi_gs/CIM/001_099/009/01.09.01_60/gs_CIM009v010901p.pdf).
- Stato del futuro core e binding HTTP: [TS 104 175](https://portal.etsi.org/webapp/WorkProgram/Report_WorkItem.asp?WKI_ID=74870), [TS 104 176](https://portal.etsi.org/webapp/WorkProgram/Report_WorkItem.asp?WKI_ID=74871). Risultano draft 0.9.6 registrati il 26 agosto 2026, non usati come baseline pubblicata.
- Framework per prove di protocollo: [suite ETSI NGSI-LD](https://forge.etsi.org/rep/cim/ngsi-ld-test-suite). Il piano richiede di fissare un commit compatibile; nessuna esecuzione della suite è rivendicata.
- Algoritmi per la valutazione del processore JSON-LD: [W3C JSON-LD 1.1 Processing Algorithms and API](https://www.w3.org/TR/json-ld11-api/).
- Motivazione tecnica dei rischi batch: [PostgreSQL 16 Transactions](https://www.postgresql.org/docs/16/tutorial-transactions.html).
- Valutazione degli indici JSONB: [PostgreSQL 16 JSON indexing](https://www.postgresql.org/docs/16/datatype-json.html#JSON-INDEXING).

Le prestazioni proposte, l'architettura dell'outbox e le soglie di rilascio sono raccomandazioni progettuali da validare, non risultati estratti dalle fonti o misurati sul broker.
