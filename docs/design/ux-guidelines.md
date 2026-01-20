# UX Design Guidelines & Philosophie

Dieses Dokument definiert die UX-Strategie für Canis. Ziel ist es, das etablierte "Gaming-Chat"-Modell (Discord/Teamspeak) durch moderne Effizienz-Patterns zu verbessern, ohne die Vertrautheit für Gamer zu opfern.

---

## Kern-Philosophie: "The Focused Hybrid"

Wir kombinieren die vertraute Struktur (Server/Channels) mit modernen Produktivitäts-Tools (Command Palette, Unified Context).

### 1. Unified Home (Der Einstiegspunkt)
Statt den Nutzer direkt in den letzten Server zu werfen, startet die App auf einem persönlichen Dashboard.
*   **Ziel:** Sofortiger Überblick über relevante Aktivitäten.
*   **Inhalt:**
    *   **Mentions & Replies:** Chronologische Liste aller direkten Ansprachen (Server-übergreifend).
    *   **Friends Online:** Wer spielt gerade was?
    *   **Active Voice:** Wo hängen meine Freunde gerade im Voice rum?

### 2. Contextual "Dynamic Island" für Voice
Voice-Chat ist ein *globaler Zustand*, kein lokaler.
*   **Problem:** In Discord muss man oft zurück zum Server scrollen/klicken, um zu sehen, wer spricht oder um sich zu muten.
*   **Lösung:** Ein persistentes Voice-Panel (z.B. unten oder schwebend), das *immer* sichtbar ist, egal wo man gerade textet.
*   **Features:** Zeigt aktiven Sprecher, Mute/Deafen Buttons, Disconnect.

### 3. Server-Agnostische Favoriten
Nutzer leben meist nur in 3-5 Channels aktiv, auch wenn sie in 50 Servern sind.
*   **Feature:** "Pinned Channels" Leiste (oben links oder separate Spalte).
*   **Funktion:** Erlaube das Anpinnen von `#general` (Server A) und `#raid-lead` (Server B) direkt untereinander.
*   **Vorteil:** Schnelles Wechseln ohne Context-Switching des ganzen Servers.

### 4. Command Palette First (`Ctrl+K`)
Maus-Navigation ist langsam.
*   **Funktion:** Ein globales Such- und Befehlsfenster.
*   **Möglichkeiten:**
    *   "Go to #channel..." (Fuzzy Search)
    *   "Mute Server..."
    *   "Change Input Device..."
    *   "Set Status..."

---

## Layout-Struktur (Entwurf)

```
+---+---------------------+------------------------------------------------+
| S |  Favorites / Quick  |                                                |
| E |  [# Gen A]          |                                                |
| R |  [# Raid B]         |                MAIN CHAT AREA                  |
| V |                     |                                                |
| E |  -----------------  |                                                |
| R |  Server Context     |                                                |
|   |  # announcements    |                                                |
| R |  # general          |                                                |
| A |  # off-topic        |                                                |
| I |                     |                                                |
| L |  Voice Channels     |                                                |
|   |  > Lobby            |                                                |
|   |                     |                                                |
+---+---------------------+------------------------------------------------+
| USR |   GLOBAL VOICE PANEL (Dynamic, Always Visible)                     |
+---+---------------------+------------------------------------------------+
```

## Persona-Checks

*   **Pippin (User):** "Ich finde sofort meine Freunde und muss nicht suchen, wo der rote Punkt herkommt."
*   **Éowyn (Dev):** "Wir müssen den `ActiveChannel` Store vom `ActiveServer` Store entkoppeln, damit Favoriten funktionieren."
*   **Gandalf (Perf):** "Die Unified Home View braucht effizientes Caching der letzten Events, damit der Start <200ms dauert."
