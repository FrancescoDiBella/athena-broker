# Athena: decisioni architetturali proposte

Data: 22 settembre 2026. Stato di tutte le decisioni: **proposto**. Sono parte del [piano](piano-evoluzione-2026-09-22.md); non descrivono modifiche già implementate.

## ADR-001 — Monolite modulare con servizi applicativi

**Problema:** i sei crate separano aree tecniche, ma ogni handler combina repository ed effetti secondari in modo diverso. La mancanza di una transazione applicativa comune si riflette in storico, batch e notifiche.

| Alternativa | Beneficio | Costo / limite |
|---|---|---|
| Continuare ad aggiungere logica agli handler | Modifiche iniziali piccole | Invarianti duplicate e regressioni tra singoli/batch |
| Monolite modulare con `athena-core` | Unità di lavoro e semantica comuni; riuso nei worker | Refactoring dei trait e contratti di mutazione |
| Microservizi separati per ogni funzione | Deploy e scaling indipendenti | Transazioni distribuite, rete, versionamento e operatività prima di conoscere i limiti reali |

**Scelta proposta:** introdurre il core applicativo mantenendo un solo workspace e PostgreSQL. Preparare un punto di avvio separato per worker quando necessario, usando gli stessi contratti e modelli. Il modello di dominio non dipende da Axum o SQLx; i tipi di protocollo sono distinti dai comandi applicativi.

**Conseguenze:** si accetta un refactoring iniziale del percorso di scrittura; in cambio diventa possibile testare una mutazione con tutti i suoi effetti. Evitare trait generici senza un caso d'uso concreto.

**Rivalutare quando:** il profiling dimostra interferenza fra API e delivery non risolvibile con budget separati, oppure i worker richiedono una capacità/deploy indipendente.

## ADR-002 — Commit di stato, storico ed evento

**Problema:** una risposta di successo attualmente non garantisce un evento recuperabile; le mutazioni correnti non vengono registrate nello storico.

| Alternativa | Beneficio | Costo / limite |
|---|---|---|
| Stato su DB, storico/notifiche su canale RAM | Minor lavoro nel commit | Perdita dopo crash; nessuna ricostruzione affidabile |
| Stato + storico + outbox nella stessa transazione | Invarianti locali e storico immediatamente disponibile | Più WAL, indici e lavoro per commit |
| Stato + log persistente, proiezione storico asincrona | Possibile riduzione del lavoro sincrono | Consistenza eventuale, lag, replay e limiti pubblici da definire |
| Log esterno come fonte primaria dell'intero broker | Replay esteso e disaccoppiamento | Ridisegno radicale delle letture e delle garanzie delle API |

**Scelta proposta:** seconda alternativa nel profilo IoT iniziale. Una revisione monotona per entità lega stato, istanze storiche ed evento. Storicizzare le istanze modificate secondo una policy esplicita. Mettere nell'evento i dati necessari a interpretare update/delete senza rileggere uno stato più recente.

**Conseguenze:** WAL e costo delle scritture aumentano rispetto al broker attuale, che non fornisce gli stessi effetti. Il benchmark deve rendere visibile questa differenza. Pool, transazioni e batch sono limitati in dimensione e durata.

**Rivalutare quando:** i test comparativi dimostrano che lo storico sincrono impedisce il target concordato e il prodotto accetta un ritardo di visibilità misurato. In quel caso il log rimane durevole, con cursori idempotenti e conservazione sufficiente per il recupero.

## ADR-003 — Outbox PostgreSQL prima di una piattaforma di streaming

**Problema:** matching e delivery devono sopravvivere ai riavvii, isolare i destinatari e distribuire il lavoro con ordine e controllo della concorrenza.

| Alternativa | Beneficio | Costo / limite |
|---|---|---|
| Canali Tokio e task liberi | Semplicità | Nessuna persistenza, limiti e ownership insufficienti |
| Outbox e delivery job PostgreSQL | Commit atomico e un solo sistema da operare | Polling, contesa, bloat e capacità condivisa col broker |
| Outbox + relay/CDC verso sistema di messaggistica | Consumer indipendenti e buffer esteso | Servizio aggiuntivo, lag del relay, duplicati e monitoraggio |

**Scelta proposta:** outbox e job persistenti con indice sul lavoro pronto, claim in batch, lease/fencing, backoff e retention. Il claim si chiude prima dell'I/O remoto. Worker con ownership per partizione preservano la sequenza dove richiesta; una chiave univoca protegge la materializzazione dei job. Conservare cursor e catalogo delle subscription in modo recuperabile.

**Conseguenze:** promettere consegna almeno una volta nel perimetro e nella retention dichiarati. Un crash dopo la ricezione del destinatario ma prima dell'ack può produrre duplicati: usare notification ID stabile e documentare l'idempotenza del consumer. Un endpoint irraggiungibile a tempo indefinito non può essere dichiarato consegnato.

**Rivalutare quando:** al picco concordato il lag continua a crescere nonostante tuning e worker sufficienti, oppure l'outbox consuma una quota misurata inaccettabile di CPU/I/O del DB; anche più consumer indipendenti con retention diverse possono giustificare il relay. Fissare la soglia nel capacity report. Non scegliere un prodotto esterno sulla sola promessa di throughput.

## ADR-004 — Storage evolutivo e storico indipendente

**Problema:** JSONB facilita la flessibilità ma può amplificare le scritture di entità grandi; lo storico attuale confonde timestamp e identità dell'istanza e dipende dal current state nelle query collection.

| Alternativa | Beneficio | Costo / limite |
|---|---|---|
| JSONB corrente + storico canonico + proiezioni selettive | Percorso incrementale e letture semplici | Contesa per entità e riscrittura del documento |
| Righe per attributo/dataset corrente + storico | Scritture più localizzate e identità esplicita | Più righe/indici e ricomposizione delle entità |
| Database storico separato | Scaling e tecniche analitiche indipendenti | Ingestione/proiezione distribuita, duplicazione e consistenza |

**Scelta proposta:** prima correggere identità e payload temporali su PostgreSQL; mantenere lo stato corrente JSONB con modello canonico. Misurare in W06 la seconda alternativa per entità grandi/calde. Se adottata, definire il nuovo storage autorevole e trattare eventuali snapshot come proiezioni versionate.

**Conseguenze:** catalogo temporale autonomo, tenant in tutte le chiavi, dataset di default gestito esplicitamente, instanceId e tempi separati. Il partizionamento non deve rompere l'unicità globale o gli aggiornamenti di istanze storiche. La perdita già avvenuta non si risolve con una migrazione di schema.

**Rivalutare quando:** WAL per update, lock wait o costo di retention superano il budget, oppure la ricomposizione degli attributi peggiora le letture oltre lo SLO. Confrontare partitioning PostgreSQL e TimescaleDB su dati reali; un secondo database richiede un beneficio ulteriore dimostrato.

## ADR-005 — Semantica comune e confine JSON-LD esplicito

**Problema:** query SQL, matching in memoria e serializzazione operano su chiavi compatte e trasformazioni diverse.

| Alternativa | Beneficio | Costo / limite |
|---|---|---|
| Mapping di stringhe personalizzato attuale | Piccolo e rapido sui casi semplici | Incompletezza JSON-LD e ambiguità fra contesti |
| Modello canonico e processore conforme verificato | Identità coerenti in storage, query e notifiche | Validazione, costo CPU e migrazione dati |
| Espansione JSON-LD ad ogni accesso al DB | Minore migrazione iniziale | Lavoro ripetuto e dipendenza da contesti remoti nel percorso frequente |

**Scelta proposta:** seconda alternativa, con espansione ai confini e compattazione in uscita. Cache per context/versione, client di uscita controllato e suite W3C pertinente. Un piano di query comune alimenta SQL e matching; confrontare i risultati con test differenziali, inclusi null, missing, numeri, array, relazioni e geometrie.

**Conseguenze:** migrazione con mappatura delle chiavi legacy; contesti irrisolvibili producono errori espliciti. Non collegare semplicemente il resolver attuale senza affrontare i limiti semantici e di uscita HTTP. Canonico significa un modello interno coerente: non richiede di salvare un documento JSON-LD espanso completo a ogni update.

**Rivalutare quando:** il profilo CPU mostra un costo dominante di espansione/compattazione dopo cache e batching; ottimizzare il processore preservando il corpus di conformance.
