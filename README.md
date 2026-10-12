# Minecraft EggWars Diorama

Diorama estático inspirado en Minecraft 1.8, renderizado con un CPU raytracer propio escrito en Rust.

**Autor:** Alejandro Pérez — **Carné:** 24136

[English version](README.en.md)

## Descripción

Este proyecto renderiza el resultado de una partida de EggWars: cinco islas flotantes generadas
proceduralmente, puentes parcialmente destruidos, defensas de obsidiana rotas, cofres abiertos,
flechas clavadas, huevos sobrevivientes y destruidos, y lava bajo un cielo de atardecer a noche.

**No es un clon jugable de Minecraft.** No hay jugador, física ni mecánicas de juego. La escena es
estática y se explora desde fuera con una cámara orbital o una cámara espectador libre. Todo el
cálculo de imagen (generación de rayos, intersecciones, sombras, reflexión, refracción) ocurre en
la CPU; no se usa GPU.

## Vista previa

![Vista previa del diorama](assets/preview.png)

## Tecnologías utilizadas

- Rust estable (edición 2024); sin `unsafe` en el renderer.
- Única dependencia: `raylib` 6.0.0, usada solo para ventana, entrada, tiempo y para mostrar el
  framebuffer ya calculado en CPU (`src/app/app.rs`). No se usa para ningún cálculo de render.
- CPU raytracer propio: `Vec3`, `Ray`, AABB, `VoxelGrid`, materiales, iluminación y entorno.
- Multithreading con la biblioteca estándar (`std::thread::scope`, `AtomicUsize`, `Mutex`).
- Texturas de 16×16 en PPM P6, cargadas por código propio. Los PNG originales de estilo
  Minecraft clásico se convierten offline con `scripts/prepare_assets.sh`.

## Arquitectura

```text
Procedural World
      |
      v
VoxelGrid + AABB Objects
      |
      v
Hybrid DDA / AABB Traversal
      |
      v
CPU Raytracer
      |
      v
Framebuffer
      |
      v
Raylib Window
```

| Módulo | Responsabilidad |
| --- | --- |
| `math`, `ray`, `geometry` | `Vec3`, `Ray`, AABB con intersección por slabs |
| `voxel` | `VoxelGrid` denso global, `BlockType`, traversal 3D DDA |
| `world` | Ruido determinista, islas, arena de cinco islas, puentes, estructuras y decoraciones |
| `scene` | Consultas híbridas voxel + AABB (hit más cercano y oclusión) |
| `material` | Materiales canónicos, texturas PPM, mapa normal, `MaterialId`/`TextureId` |
| `lighting`, `environment` | Luz ambiental, direccional y puntual; cielo procedural |
| `render` | Trazado recursivo, tiles y trabajadores en paralelo, framebuffer |
| `camera` | `OrbitalCamera`, `FreeCamera` y el trait `ViewCamera` |
| `app` | Ventana, entrada y presentación con Raylib |

## Decisiones de diseño

| Decisión | Justificación |
| --- | --- |
| `Vec3` y `Ray` propios | Control de convenciones numéricas y sin dependencias externas. |
| Geometría AABB-first | Los bloques, cofres, flechas y huevos son cajas alineadas a los ejes; encaja con el estilo pixelado. |
| Un `VoxelGrid` denso global | Una sola estructura contigua para las cinco islas; almacena solo la identidad del bloque (1 byte). |
| Traversal 3D DDA | Recorre únicamente las celdas cruzadas por el rayo, en lugar de probar cada bloque. |
| Renderizado híbrido voxel/AABB | Terreno y construcciones en voxels; props pequeños como AABB. Gana el hit más cercano. |
| Seeds deterministas | Hash sin estado por dominio (sin RNG compartido): la misma seed produce la misma escena. |
| Texturas en CPU, nearest-neighbor | Conserva el aspecto pixelado; el muestreo no depende de Raylib. |
| `MaterialId` / `TextureId` | Los materiales y texturas se comparten por identificador, sin duplicar datos por bloque. |
| Reflexión recursiva acotada | Profundidad máxima fija (`MAX_RAY_DEPTH = 3`) para controlar el costo. |
| Refracción de Snell y Fresnel | El vidrio (IOR 1.5) mezcla reflexión y refracción con la aproximación de Schlick. |
| Luz direccional y puntual | Sol direccional más luces puntuales finitas para la lava. |
| Sombras duras | Un rayo de sombra por luz; simple y predecible. |
| Lava emisiva | Emisión propia del material más una luz puntual representativa (no una por voxel). |
| Normal mapping de cobblestone | Mapa normal derivado offline de su propia textura; solo afecta la iluminación local. |
| Skybox procedural | Atardecer a noche evaluado en espacio de mundo; es el único camino de rayos que fallan. |
| Renderizado por tiles determinista | Cada píxel usa sus coordenadas globales; la imagen no depende del orden ni del número de hilos. |
| Multithreading con `std::thread::scope` | Los hilos reclaman tiles de 16×16 con un contador atómico; sin pools ni `unsafe`. |
| Agrupación espacial de AABB decorativos | 8 grupos con bounds exactos descartan rayos que no los tocan; resultado idéntico al escaneo lineal. |
| Cámara orbital y espectador libre | Ambas implementan `ViewCamera`; el renderer es el mismo para las dos. |

## Escena EggWars

- Cuatro islas exteriores de 16×16 y una isla central de 24×24, con terreno procedural de pasto,
  tierra y roca, y minerales (carbón, hierro, oro, diamante) distribuidos de forma determinista.
- Cuatro puentes en distintos estados: completo (Norte), dañado (Este), incompleto (Oeste) y
  cortado (Sur).
- Defensas de obsidiana de nueve bloques alrededor de cada huevo: intacta (Norte), dañada (Oeste),
  perforada (Este) y destruida (Sur).
- Huevos: uno sobrevive en la isla Oeste; el de la isla Norte está sellado dentro de su defensa
  intacta. Fragmentos de cascarón en las islas Este y Sur.
- Una ruina central con piso de obsidiana, muros parciales, ventanas de vidrio, dos cofres abiertos
  y flechas clavadas en muros, defensas y puentes; escombros repartidos por la arena.
- Un pozo de lava de 2×2 en la ruina, con una luz puntual cálida, y un derrame de lava sobre el
  borde sur de la isla central.
- Cielo procedural de atardecer a noche.

La narrativa es ambiental: los puentes rotos, las defensas perforadas y los objetos abandonados
sugieren un asalto que ya terminó, con un huevo que resistió y otros que no.

## Materiales

| Material | Efecto que demuestra |
| --- | --- |
| Grass | Texturas por cara (superior, lateral, inferior) con color de bioma aplicado offline. |
| Cobblestone | Normal mapping derivado de su propia textura. |
| Obsidian | Reflexión recursiva sobre una superficie oscura. |
| Glass | Refracción de Snell con composición de Fresnel. |
| Lava | Emisión propia e iluminación local mediante luz puntual. |

Son aproximaciones de tiempo real razonable, no una simulación físicamente exacta.

## Requisitos de la rúbrica

| Requisito | Implementación | Estado |
| --- | --- | --- |
| CPU raytracing | Renderer propio, sin GPU | Implementado |
| Terreno procedural de al menos 16×16 | Islas 16×16 y 24×24 con seed `u64` | Implementado |
| Cinco materiales con textura | Grass, cobblestone, obsidian, glass, lava | Implementado |
| Reflexión | Obsidian, recursiva | Implementado |
| Refracción | Glass, Snell + Fresnel | Implementado |
| Normal mapping | Cobblestone | Implementado |
| Material emisivo | Lava, con luz puntual | Implementado |
| Skybox | Entorno procedural atardecer/noche | Implementado |
| Rotación orbital y zoom | `OrbitalCamera` | Implementado |
| Cámara libre (adicional) | `FreeCamera` espectador | Implementado |
| Renderizado paralelo y optimización | Tiles + hilos, `VoxelGrid` + 3D DDA, agrupación espacial | Implementado |
| Escena compleja y atractiva | Diorama EggWars de cinco islas | Implementado |
| Video de demostración | [YouTube](https://youtu.be/FagbaJLKZds) | Entregado |

## Ejecución

Requiere Rust estable y Cargo (macOS; Raylib se compila como dependencia). Los PPM de texturas ya
están en `assets/textures/`; solo se necesita `scripts/prepare_assets.sh` para regenerarlos desde
`assets/source/` (usa `sips` de macOS).

```bash
cargo run --release -- --eggwars --seed 1847
cargo run --release -- --eggwars --seed 42
cargo test
```

La seed por defecto de `--eggwars` es 42. La misma seed genera siempre la misma escena. Al iniciar,
la imagen inicial se guarda en `output/eggwars_seed_<seed>.ppm`. Otros modos de diagnóstico:
`cargo run --release` (showcase de cinco bloques), `-- --voxel-diagnostic` y
`-- --procedural-island [--seed <u64>]`.

## Controles

| Cámara orbital (por defecto) | |
| --- | --- |
| Flechas | Orbitar (horizontal y elevación) |
| `W` / `S` o rueda del mouse | Acercar / alejar |
| `C` | Cambiar a cámara libre |
| `Esc` | Salir |

| Cámara libre (espectador, noclip) | |
| --- | --- |
| `W` / `S` | Avanzar / retroceder |
| `A` / `D` | Desplazar a izquierda / derecha |
| `Espacio` / `Shift` | Subir / bajar |
| Mouse | Mirar (el cursor se captura solo en este modo) |
| `C` | Volver a la cámara orbital, restaurada tal como estaba |
| `Esc` | Salir |

La cámara libre comienza en la posición y dirección exactas de la orbital. No tiene física,
gravedad ni colisiones.

## Rendimiento

Mediciones locales (Apple M3 Pro, 11 CPUs, modo release, seed 1847); no son benchmarks universales.

| Concepto | Valor |
| --- | --- |
| Resolución interna / ventana | 640×360 / 1280×720 (escala 2×, nearest-neighbor) |
| Vista inicial, hilos automáticos | ≈ 10 ms |
| Vista inicial antes del pozo y derrame de lava, 1 / 4 / auto hilos | 61 / 16 / 8 ms |
| Tests automatizados | 743 pasando |

Los fotogramas sin cambios de cámara no se vuelven a renderizar. Más detalle histórico en
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Limitaciones conocidas

- El vidrio y la lava proyectan sombras opacas; no hay seguimiento de medios anidados (el vidrio
  junto a otro bloque puede mostrar su cara interior).
- Profundidad de recursión fija (3).
- La geometría decorativa está alineada a los ejes (sin tapas inclinadas ni flechas en ángulo).
- Sin antialiasing.
- Sin gameplay, por diseño.
- Sin colisión de cámara: la orbital puede entrar en una isla y la libre atraviesa todo (noclip).
- El huevo de la isla Norte está sellado y no se instancia en la escena.

## Video de demostración

[![Video de demostración](https://img.youtube.com/vi/FagbaJLKZds/maxresdefault.jpg)](https://youtu.be/FagbaJLKZds)

Enlace: <https://youtu.be/FagbaJLKZds>

El video muestra la vista inicial con las cinco islas, la órbita y el zoom, un acercamiento a la
ruina central (lava, cofres, flechas, vidrio y obsidiana), los puentes y defensas dañados, el cambio
a la cámara libre y la variación entre seeds.

## Historial de desarrollo

Resúmenes de misiones, decisiones y mediciones históricas: [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Autoría

Alejandro Pérez — Carné: 24136. Proyecto académico de Computación Gráfica.
