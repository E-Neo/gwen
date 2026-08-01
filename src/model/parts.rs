use crate::opc::Package;

/// Resolve the theme part URI (e.g. `ppt/theme/theme1.xml`), if present.
pub fn theme_uri(pkg: &Package) -> Option<String> {
    let rels = pkg.get_rels("ppt/presentation.xml")?;
    for rel in rels.values() {
        if rel.rel_type.contains("theme") {
            return pkg.resolve_relationship_target("ppt/presentation.xml", rel);
        }
    }
    None
}

/// Resolve the slide master part URIs, in relationship order.
pub fn master_uris(pkg: &Package) -> Vec<String> {
    let mut out = Vec::new();
    let Some(rels) = pkg.get_rels("ppt/presentation.xml") else {
        return out;
    };
    for rel in rels.values() {
        if rel.rel_type.contains("slideMaster")
            && let Some(uri) = pkg.resolve_relationship_target("ppt/presentation.xml", rel)
        {
            out.push(uri);
        }
    }
    out
}

/// Resolve all slide layout part URIs (walking each master's relationships).
pub fn layout_uris(pkg: &Package) -> Vec<String> {
    let mut out = Vec::new();
    for master in master_uris(pkg) {
        let Some(rels) = pkg.get_rels(&master) else {
            continue;
        };
        for rel in rels.values() {
            if rel.rel_type.contains("slideLayout")
                && let Some(uri) = pkg.resolve_relationship_target(&master, rel)
            {
                out.push(uri);
            }
        }
    }
    out
}
