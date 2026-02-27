# <span style="color: #88C0D0;">Deploying Your Pack (Self-Hosting Kaiku)</span>

> [!WARNING]  
> **Early Development Note:** Kaiku is heavily in development. The architecture and deployment strategies described here represent our current structural roadmap and are actively being built. They are not yet ready for production deployment.

Kaiku was built from day one to reject corporate data harvesting. Self-hosting is not a second-class feature; it is the **primary** way Kaiku is meant to be experienced. 

This guide will (eventually) cover everything you need to know to deploy Kaiku for your community, whether you are a group of five friends or a massive esports organization.

---

## <span style="color: #88C0D0;">Core Philosophy</span>
If you bring the hardware, you own the data. 

Unlike platforms like Discord or TeamSpeak, a self-hosted Kaiku node grants you absolute sovereignty over:
- Voice metadata and analytics
- Direct message encryption keys
- Server roles and community metrics
- Uptime SLA and routing

## <span style="color: #88C0D0;">Infrastructure Overview</span>
A standard Kaiku deployment consists of three primary services:

1. **The Kaiku Client (Desktop/Web)**: The fast, Tauri-based application that your users actually interact with.
2. **The Signaling Server (Node)**: A lightweight WebSocket server responsible for matching peers and initiating WebRTC handshakes. 
3. **TURN/STUN Relays**: Necessary for bypassing strict NATs and firewalls so your users can always connect directly to each other.

### Recommended Specs (Initial Estimate)
Because the Heavy Lifting (Voice Encoding/Decoding) is handled client-side via WebRTC, the server footprint is incredibly small.
- **CPU**: 1-2 vCores
- **RAM**: 1GB (Signaling is incredibly efficient)
- **Bandwidth**: Dependent on TURN relay usage.

---

## <span style="color: #88C0D0;">Deployment Scenarios (Coming Soon)</span>

### 1. The "Lone Wolf" (Docker Compose)
The fastest way to get a node online. We will provide a pre-configured `docker-compose.yml` that spins up the Signaling Server, a coturn instance (for relaying), and a Redis cache in a single command. 

### 2. The "Esports Org" (Kubernetes)
For massive communities. Detailed Helm charts and documentation for deploying scalable replicas of the Signaling server behind a load balancer, with dedicated external TURN clusters.

---
*Documentation to be expanded as the implementation stabilizes.*
