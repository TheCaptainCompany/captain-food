<img src="https://r2cdn.perplexity.ai/pplx-full-logo-primary-dark%402x.png" style="height:64px;margin-right:32px"/>

# Add these people referenced as intellectual frameworks in this discussion:

Domain \& Architecture experts
Eric Evans — creator of DDD, coined the term Anti-Corruption Layer
Greg Young — creator of CQRS and Event Sourcing, EventStoreDB/KurrentDB
Vaughn Vernon — author of Implementing Domain-Driven Design and Reactive Messaging Patterns with the Actor Model
Kent Beck — creator of TDD, xUnit, Extreme Programming
Jeff Patton — creator of User Story Mapping
---
I would like to discuss about the UI spec in yaml, does it exists? I heard about ways to work around the constraints of upgrade new version of apps on iPhone by declaring the spec of the app on the server then with a UI framework that understands the spec display the pages/screens with the right positions. For me it look likes html or XAML but there is something that I guess it's more than that. I don't remember the name of this technics. Server side rendering? or Server side something...

J'ai tout ce qu'il me faut. Voici maintenant ma réponse complète, en deux parties : les références intellectuelles demandées, puis le sujet SDUI.

***

## Références intellectuelles — Domain \& Architecture Experts

### Eric Evans

Auteur de *Domain-Driven Design: Tackling Complexity in the Heart of Software* (Addison-Wesley, 2003) — le "Blue Book". Evans travaille depuis les années 1990 sur des systèmes métier complexes et a formalisé les concepts fondateurs du DDD : Ubiquitous Language, Bounded Context, Anti-Corruption Layer, Core Domain. Il dirige Domain Language Inc., une société de conseil et formation, et reste actif dans la communauté via les conférences DDD Europe, Explore DDD, et KanDDDinsky.[^1][^2][^3]

### Greg Young

Inventeur du terme **CQRS** (Command Query Responsibility Segregation) et pionnier de l'**Event Sourcing** comme pratique formalisée. Il a développé ces idées en travaillant sur des systèmes de trading algorithmique qui nécessitaient un audit log déterministe. Il est également le créateur d'**EventStoreDB**, une base de données fonctionnelle dédiée à l'Event Sourcing, renommée **KurrentDB**. Consultant indépendant avec un background allant des systèmes embarqués aux systèmes d'entreprise.[^4][^5][^6]

### Vaughn Vernon

Auteur de *Implementing Domain-Driven Design* (le "Red Book") et *Reactive Messaging Patterns with the Actor Model*, tous deux publiés chez Addison-Wesley. Là où Evans explique le *pourquoi*, Vernon montre le *comment* — avec des patterns, du code et des exemples architecturaux concrets couvrant microservices, CQRS, Domain Events, et messaging. Plus de 30 ans d'expérience en design et architecture logicielle.[^7][^8]

### Kent Beck

**Correction à faire ici** : Beck se décrit lui-même comme le **"rediscoverer"** de TDD, pas strictement son inventeur — les pratiques de test-first existaient avant, notamment dans la communauté Smalltalk. Ce qu'il a fait : il a formalisé et popularisé TDD comme méthodologie de design (pas seulement de test), a écrit le premier framework xUnit (SUnit pour Smalltalk), puis JUnit avec Erich Gamma. Il est également le créateur d'**Extreme Programming (XP)** (1999) et l'un des 17 signataires originaux du Manifeste Agile. Son livre *Test-Driven Development: By Example* (2002) reste la référence canonique.[^9][^10][^11][^12][^13]

### Jeff Patton

Créateur de la technique **User Story Mapping**, développée de 2005 à 2014 pour répondre au problème des backlogs plats qui font perdre la vue d'ensemble. Auteur du livre *User Story Mapping* (O'Reilly), bestseller. Gagnant du Gordon Pask Award 2007 de l'Agile Alliance. Son approche mêle Agile, Lean Startup, UX et Design Thinking avec une obsession centrale : construire des produits utiles plutôt que simplement livrer vite.[^14][^15][^16][^17]

***

## Server-Driven UI (SDUI) — ce dont tu parles

Le terme que tu cherches est **Server-Driven UI (SDUI)**, parfois appelé **Backend-Driven UI**. Ce n'est pas du Server-Side Rendering (SSR) — une confusion fréquente.

### La différence clé

| Concept | Ce qui arrive côté serveur | Ce que le client reçoit |
| :-- | :-- | :-- |
| **SSR** (Next.js, Rails…) | Le serveur génère du HTML | Du HTML déjà rendu |
| **SDUI** | Le serveur envoie la *structure* de l'UI | Un JSON/YAML décrivant des composants natifs |

Avec SSR, tu reçois du HTML que WebView affiche. Avec SDUI, l'app native reçoit une description de l'interface et la **rend elle-même nativement** — SwiftUI côté iOS, Kotlin côté Android — ce qui préserve les performances et l'UX native.[^18]

### Le problème que ça résout

L'App Store d'Apple impose une revue de chaque version, ce qui prend 24–48h minimum et force les utilisateurs à mettre à jour. Avec SDUI, l'app embarque tous les composants possibles au moment de la soumission, mais leur **arrangement, contenu et comportement viennent du serveur** en temps réel. Résultat : changer un écran = changer une réponse JSON sur le backend. Zéro re-soumission.[^19][^18]

### Le format : JSON, pas YAML

En pratique, le format dominant est **JSON**, pas YAML — même si techniquement rien n'interdit YAML (les deux sont des formats de sérialisation, YAML étant un superset de JSON). JSON s'impose pour deux raisons pratiques : performance de parsing côté mobile et outillage API universel. Voici à quoi ça ressemble concrètement  :[^20][^21]

```json
{
  "type": "screen",
  "components": [
    { "type": "hero_banner", "imageUrl": "...", "title": "Offre du jour" },
    { "type": "button", "label": "Commander", "action": { "type": "navigate", "destination": "/checkout" } }
  ]
}
```

L'app iOS/Android contient un **registre de composants** qui mappe chaque `"type"` vers une vue SwiftUI ou Compose native.[^18]

### Qui l'utilise en prod

Le cas le plus documenté est celui d'**Airbnb** avec leur **Ghost Platform** : un schéma GraphQL unique drive search, listings et checkout sur web, iOS et Android simultanément — sans mise à jour store. Les éléments fondamentaux sont *Sections* (blocs UI indépendants et réutilisables), *Screens* (organisation des sections en layout), et *Actions* (comportements utilisateur). Spotify, Netflix, Lyft, Swiggy, et Flipkart utilisent des variantes similaires.[^22][^23][^19]

Des frameworks open-source existent aussi, notamment **Beagle** (ZupIT, cross-platform iOS/Android)  et **DivKit** (Yandex/Spotify).[^24][^19]

### Limites réelles à anticiper

SDUI n'est pas gratuit : la complexité se déplace du client vers le backend, qui doit gérer le **versioning du schéma** (une vieille version de l'app doit toujours pouvoir rendre les nouveaux composants de façon dégradée), les performances réseau (gros payloads = latence visible), et la coordination accrue entre équipes mobile et backend. C'est un investissement justifié à partir d'une certaine fréquence de changements UI et d'un parc d'utilisateurs qui se met lentement à jour — exactement le problème que tu as probablement chez Captain.Food sur les versions déployées.[^25][^26]
<span style="display:none">[^27][^28][^29][^30][^31][^32][^33][^34][^35][^36][^37][^38][^39][^40][^41][^42][^43][^44][^45][^46][^47][^48][^49][^50][^51][^52][^53][^54][^55][^56][^57][^58][^59][^60][^61][^62][^63][^64][^65][^66][^67][^68][^69][^70][^71][^72][^73][^74][^75][^76][^77][^78][^79][^80][^81][^82][^83][^84][^85][^86][^87][^88][^89][^90]</span>

<div align="center">⁂</div>

[^1]: https://www.avanscoperta.it/en/trainer/eric-evans/

[^2]: https://gotocon.com/berlin-2015/speaker/Eric+Evans

[^3]: https://www.linkedin.com/in/ericevansddd

[^4]: https://nestenius.se/net/how-to-use-kurrentdb-for-event-sourcing-in-c-on-azure/

[^5]: https://virtualddd.com/videos/greg-young-a-decade-of-ddd-cqrs-event-sourcing/

[^6]: https://www.infoq.com/news/2014/09/greg-young-event-sourcing/

[^7]: https://medium.com/@stoic.engineer/book-review-implementing-domain-driven-design-2d40da61340b

[^8]: https://virtualddd.com/videos/the-language-of-actors-vaughn-vernon/

[^9]: https://www.youtube.com/watch?v=aSXaxOdVtAQ

[^10]: https://www.youtube.com/watch?v=guycIP56YeY

[^11]: https://demo.pyrite.wiki/site/agile/kent-beck

[^12]: https://dayton.fed.wiki/kent-beck.html

[^13]: https://www.youtube.com/watch?v=C5IH0ABmyc0

[^14]: https://www.linkedin.com/in/productdesigncoach

[^15]: https://www.infoq.com/interviews/agile2015-patton/

[^16]: https://www.youtube.com/watch?v=AorAgSrHjKM

[^17]: https://en.wikipedia.org/wiki/User_story

[^18]: https://pyramidui.com/blog/sdui-tutorial-swiftui/

[^19]: https://www.netclues.com/blog/update-app-without-app-store-approval

[^20]: https://medium.com/@sanjaykumawat94222/implementing-server-driven-ui-for-ios-e5f65fcc8fba

[^21]: https://pyramidui.com/blog/server-driven-ui-swiftui-ios-guide

[^22]: https://www.youtube.com/watch?v=LLQw8chckyg

[^23]: https://www.infoq.com/news/2021/07/airbnb-server-driven-ui/

[^24]: https://github.com/ZupIT/beagle

[^25]: https://www.slideshare.net/slideshow/server-driven-ui-in-ios/253017191

[^26]: https://ijesm.co.in/uploads/68/15375_pdf.pdf

[^27]: 20260627-ADR-019 Google Business Profile Order Button.md

[^28]: 20260627-Captain.Food — Nouveaux ADRs Juin 2026.md

[^29]: 20260627-captain_food_adrs_ubereats_comparison_june2026.md

[^30]: 20260628-claude-code-adr-observability-playbook.md

[^31]: THE_CAPTAIN_COMPANY_ADR_181225.md

[^32]: captain-group-adr.md

[^33]: Mon histoire.docx

[^34]: BUSINESS_MODEL_FINAL_-_CAPTAIN.food__captain_startup__divino.pdf

[^35]: ADR Captain.Food Phase 2.pdf

[^36]: 20251120-captain-food-adrs-qas-update.md

[^37]: 20251127-captain_food_qas_final.md

[^38]: 20251127-captain_food_adrs_final.md

[^39]: 20251127-captain_food_business_model_canvas_final.md

[^40]: 20251127-captain_food_story_mapping_final.md

[^41]: 202251119-01h10-captain-food-brand-identity.md

[^42]: 20251112-expansion-strategy-deferred.md

[^43]: 20251112-captain-decisions-focus-tours-nov12.md

[^44]: 20251112_19h29-updated-decisions-nov12.md

[^45]: 20251112_19h26-pos-analysis.md

[^46]: 20251112_19h26-mvp-final-projections.md

[^47]: https://mobikul.com/rendering-widgets-using-json-in-flutter/

[^48]: https://azimmemon2002.github.io/blog/dynamic-mobile-ui-updates-without-play-store-updates/

[^49]: https://medium.com/@moha97ibrahim/server-driven-user-interface-swiftui-9f6412615b5f

[^50]: https://medium.com/@ios-interview/server-driven-ui-vs-static-ui-in-ios-development-ec7229bd1506

[^51]: https://pyramidui.com/blog/sdui-vs-cross-platform/

[^52]: https://nativeblocks.io/blog/server-driven-ui-pros-cons/

[^53]: https://nativeblocks.io/sdui/

[^54]: https://yaml.org/spec/1.2.1/

[^55]: https://habr.com/ru/companies/alconost/articles/568444/

[^56]: https://mobile-vitals.com/article/1178-airbnb-a-deep-dive-into-airbnb-s-server-driven-ui-system

[^57]: https://medium.com/airbnb-engineering/a-deep-dive-into-airbnbs-server-driven-ui-system-842244c5f5

[^58]: https://pyramidui.com/blog/post.html?p=why-airbnb-lyft-netflix-use-sdui

[^59]: https://www.infoq.com/jp/news/2021/08/airbnb-server-driven-ui/

[^60]: https://brunch.co.kr/@advisor/37

[^61]: https://pyramidui.com/blog/why-airbnb-lyft-netflix-use-sdui

[^62]: https://www.hojunin.com/contents/server-driven-ui-deep-dive-airbnb

[^63]: http://www.benjaminoakes.com/programming/2021/07/21/A-Deep-Dive-into-Airbnbs-ServerDriven-UI-System/

[^64]: https://www.linkedin.com/posts/anshul-kahar_tired-of-publishing-minor-release-for-small-activity-7374073042274222080-Kv-W

[^65]: https://www.linkedin.com/posts/harshit-sachan-334a50174_flutter-serverdrivenui-sdui-activity-7351838144461225984-ZoAG

[^66]: https://medium.com/@rbro112

[^67]: https://www.okoone.com/spark/product-design-research/changing-user-interface-with-airbnbs-ghost-platform/

[^68]: https://www.informit.com/articles/article.aspx?p=2023702

[^69]: http://api.tc-skill.ru/media/admin_panel_files/Vaughn_Vernon_-_Implementing_Domain-Driven_Design_proglib.pdf

[^70]: https://exploreddd.com/2017/speakers/eric-evans.html

[^71]: https://www.youtube.com/watch?v=vAi5gRrqVMk

[^72]: https://www.infoq.com/presentations/strategic-design-evans/

[^73]: https://www.oreilly.com/library/view/implementing-domain-driven-design/9780133039900/

[^74]: https://2026.dddeurope.com/speakers/eric-evans/

[^75]: https://www.fnac.com/a5374870/Vaughn-Vernon-Implementing-Domain-Driven-Design

[^76]: https://www.infoq.com/interviews/eric-evans-ddd-interview/

[^77]: https://en.wikipedia.org/wiki/Kent_Beck

[^78]: https://ueberproduct.de/en/episode-7-jeff-patton-user-story-maps/

[^79]: https://open.spotify.com/episode/6PjPiSqgyPFYknwwAc82oH

[^80]: https://publish.obsidian.md/pkc/Literature/People/Kent+Beck

[^81]: https://www.youtube.com/watch?v=x1O5cKCAgdk

[^82]: https://www.goodreads.com/book/show/23317497-user-story-mapping

[^83]: https://archive.oredev.org/oredev2012/2012/speakers/greg-young

[^84]: https://www.youtube.com/watch?v=LGjRfgsumPk

[^85]: https://craigjcox.com/authors/greg-young

[^86]: https://www.youtube.com/watch?v=AspkNFjhHIM

[^87]: https://learncqrs.com/do6pMI

[^88]: https://stackoverflow.com/users/58396/greg-young

[^89]: https://github.com/leandrocp/awesome-cqrs-event-sourcing/blob/master/README.md

[^90]: https://www.youtube.com/watch?v=JHGkaShoyNs

