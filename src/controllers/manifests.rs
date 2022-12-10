use std::os::unix::prelude::MetadataExt;

use axum::{response::IntoResponse, extract::{Path, BodyStream, State}, TypedHeader, headers, http::StatusCode, body::StreamBody, debug_handler};
use tracing::info;

use crate::{data::{helpers::{reject_invalid_container_refs, RegistryPathsHelper, reject_invalid_tags_refs}, manifests::{Manifest, ManifestMetadata}}, ApplicationState};
use crate::controllers::RegistryHttpResult;

use super::RegistryHttpError;

#[tracing::instrument(skip_all, fields(container_ref = container_ref, manifest_ref = manifest_ref))]
pub async fn upload_manifest(
    Path((container_ref, manifest_ref)): Path<(String, String)>,
    TypedHeader(content_type): TypedHeader<headers::ContentType>,
    State(app): State<ApplicationState>,
    mut body: BodyStream
) -> RegistryHttpResult {
    reject_invalid_container_refs(&container_ref)?;
    reject_invalid_tags_refs(&manifest_ref)?;

    let mut manifest = Manifest::new(
        &app.conf.registry_storage, 
        &app.conf.temporary_registry_storage,
        &container_ref, 
        &manifest_ref
    );

    info!("Saving manifest");
    manifest.save_manifest(&mut body).await?;
    info!("Saving metadata");
    manifest.save_manifest_metadata(&content_type.to_string()).await?;

    Ok((
        StatusCode::CREATED,
        [
            ("Location", format!("/v2/{}/manifests/{}", container_ref, manifest_ref)),
            ("Docker-Content-Digest", format!("sha256:{}", manifest.docker_hash()?))
        ]
    ).into_response())
}

#[tracing::instrument(skip_all)]
pub async fn fetch_manifest(
    Path((container_ref, manifest_ref)): Path<(String, String)>,
    State(app): State<ApplicationState>,
) -> RegistryHttpResult {
    reject_invalid_container_refs(&container_ref)?;
    reject_invalid_tags_refs(&manifest_ref)?;

    let manifest_path = RegistryPathsHelper::manifest_path(&app.conf.registry_storage, &container_ref, &manifest_ref);
    let manifest_file = match tokio::fs::File::open(&manifest_path).await {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(RegistryHttpError::manifest_not_found(&container_ref, &manifest_ref));
        }
        Err(e) => return Err(e.into())
    };
    let manifest_size = manifest_file.metadata().await?.size();

    let manifest_meta_path = RegistryPathsHelper::manifest_meta(&app.conf.registry_storage, &container_ref, &manifest_ref);
    let manifest_meta = tokio::fs::read_to_string(&manifest_meta_path).await?;
    let manifest_meta = serde_json::from_str::<ManifestMetadata>(&manifest_meta).unwrap();

    let manifest_stream = StreamBody::new(tokio_util::io::ReaderStream::new(manifest_file));

    Ok((
        StatusCode::OK,
        [
            ("Docker-Content-Digest", format!("sha256:{}", manifest_meta.hash)),
            ("Content-Type", manifest_meta.content_type.to_string()),
            ("Content-Length", manifest_size.to_string())
        ],
        manifest_stream
    ).into_response())
}

#[debug_handler]
pub async fn proxy_fetch_manifest(
    Path((container_ref, manifest_ref)): Path<(String, String)>,
    State(app): State<ApplicationState>
) -> RegistryHttpResult {
    reject_invalid_container_refs(&container_ref)?;
    reject_invalid_tags_refs(&manifest_ref)?;

    let client = app.docker_clients.get_client(&container_ref).await?;

    Ok((StatusCode::NOT_IMPLEMENTED).into_response())
}