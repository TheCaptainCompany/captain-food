//! The SDUI renderer (split 1/4 of #21).
//!
//! Every renderable node is keyed by a GENERATED [`ComponentKind`] (the spec allowlist), so a screen
//! can only reference components declared in `customer_screens.yaml#/component_registry`. This first
//! slice renders ONE static screen. It compiles two ways from the SAME view tree:
//!   * `ssr` (default, native)  — [`render_home_html`] produces the initial HTML on the server;
//!   * `hydrate` (wasm32)       — [`hydrate`] attaches the client to that server-rendered DOM.
//! Live resolvers/actions and the two-step mutation layer (#17) arrive in later splits.

use leptos::prelude::*;

use crate::generated::registry::ComponentKind;

/// A minimal static SDUI node: a registered component kind plus its literal text. The live renderer
/// will carry props/children/bindings; this skeleton proves the registry dispatch seam.
#[derive(Debug, Clone, Copy)]
pub struct StaticNode {
    pub kind: ComponentKind,
    pub text: &'static str,
}

impl StaticNode {
    pub const fn new(kind: ComponentKind, text: &'static str) -> Self {
        Self { kind, text }
    }
}

/// The one static screen this slice renders — a minimal subset of the `home` chrome. Every node is
/// dispatched through the generated registry.
const HOME_NODES: &[StaticNode] = &[
    StaticNode::new(ComponentKind::PageHeader, "Captain.Food"),
    StaticNode::new(ComponentKind::Text, "Order from independent restaurants in Tours."),
    StaticNode::new(ComponentKind::CtaBanner, "Run a restaurant? Partner with us."),
];

/// Render one node as a Leptos view, dispatching on its [`ComponentKind`]. The element shape is a
/// skeleton stand-in (the real per-component views land next); the invariant proven here is that
/// rendering is driven by the generated allowlist, each node tagged with its spec `type`.
fn node_view(node: StaticNode) -> AnyView {
    let ty = node.kind.as_str();
    let body = node.text;
    match node.kind {
        ComponentKind::PageHeader => {
            view! { <header data-c=ty><h1>{body}</h1></header> }.into_any()
        }
        ComponentKind::Text => view! { <p data-c=ty>{body}</p> }.into_any(),
        ComponentKind::CtaBanner => {
            view! { <aside data-c=ty class="cta">{body}</aside> }.into_any()
        }
        // Skeleton fallback: any other registered kind renders as a tagged block until its real view lands.
        _ => view! { <div data-c=ty>{body}</div> }.into_any(),
    }
}

/// The static `home` screen as a Leptos component. The `data-hydrate` marker on the root is where the
/// client hydration entry attaches.
#[component]
pub fn HomeScreen() -> impl IntoView {
    let nodes: Vec<AnyView> = HOME_NODES.iter().copied().map(node_view).collect();
    view! {
        <main id="app" data-hydrate="home">
            {nodes}
        </main>
    }
}

/// Server-side render the `home` screen to a full HTML document (the `ssr` build). The BFF serves this
/// as the initial response; the shipped wasm bundle then hydrates it.
#[cfg(feature = "ssr")]
pub fn render_home_html() -> String {
    // The screen is static (no signals), so rendering the view to HTML needs no reactive runtime.
    let body = HomeScreen().to_html();
    format!(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>Captain.Food</title></head><body>{body}</body></html>"
    )
}

/// Client hydration entry (the `hydrate` build, wasm32): attach the app to the server-rendered DOM.
#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    leptos::mount::hydrate_body(HomeScreen);
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    #[test]
    fn static_home_renders_registered_components_only() {
        let html = render_home_html();
        assert!(html.contains("<title>Captain.Food</title>"));
        assert!(html.contains("data-hydrate=\"home\""));
        // Every rendered component tag is a member of the generated allowlist.
        assert!(html.contains("data-c=\"page_header\""));
        assert!(html.contains("data-c=\"text\""));
        assert!(html.contains("data-c=\"cta_banner\""));
    }

    #[test]
    fn registry_allowlist_round_trips() {
        for kind in ComponentKind::ALL {
            assert_eq!(ComponentKind::from_type(kind.as_str()), Some(*kind));
        }
        assert_eq!(ComponentKind::from_type("not_a_component"), None);
    }
}
