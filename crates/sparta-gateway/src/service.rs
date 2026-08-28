//! The Sparta S3 service: a wrapper around the upstream proxy that adds
//! shadow-mode observation now and the dedup engine at M4, under Invariant 0:
//!
//! 1. Every added behavior is wrapped so its failure (including panics) falls
//!    back to plain proxying; the customer's request never fails because
//!    Sparta failed.
//! 2. `SPARTA_BYPASS=1` short-circuits everything to a dumb proxy, checked
//!    per request.
//! 3. Unsupported/unhandled operations delegate straight to the upstream via
//!    `s3s-aws`'s full-surface proxy — they were never "ours" to break.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use async_trait::async_trait;
use futures::FutureExt;
use s3s::{S3Request, S3Response, S3Result, S3};
use tracing::{error, warn};

use crate::config::{bypass_requested, GatewayConfig};
use crate::db::Db;
use crate::shadow;

pub struct SpartaS3 {
    pub inner: s3s_aws::Proxy,
    pub db: Arc<Db>,
    pub cfg: Arc<GatewayConfig>,
}

impl SpartaS3 {
    /// Count an op; never fails a request over stats.
    fn note(&self, op: &'static str) {
        if let Err(e) = self.db.bump(op, 1) {
            warn!(op, error = %e, "op counter bump failed");
        }
    }

    /// Run added (non-essential) logic; a panic inside is logged and ignored.
    fn guarded<F: FnOnce()>(&self, what: &'static str, f: F) {
        if std::panic::catch_unwind(AssertUnwindSafe(f)).is_err() {
            error!(what, "internal logic panicked; continuing as passthrough");
        }
    }
}

/// Await the upstream call with a panic net: a panic below us becomes a clean
/// 500 instead of tearing down the connection task.
async fn proxied<T>(fut: impl std::future::Future<Output = S3Result<T>>) -> S3Result<T> {
    match AssertUnwindSafe(fut).catch_unwind().await {
        Ok(result) => result,
        Err(_) => {
            error!("proxy call panicked");
            Err(s3s::s3_error!(InternalError, "internal panic"))
        }
    }
}

#[async_trait]
impl S3 for SpartaS3 {
    // ---- Intercepted data-path operations --------------------------------

    async fn put_object(
        &self,
        mut req: S3Request<s3s::dto::PutObjectInput>,
    ) -> S3Result<S3Response<s3s::dto::PutObjectOutput>> {
        self.note("op_put_object");
        if !bypass_requested() && self.cfg.shadow_chunking {
            let db = Arc::clone(&self.db);
            self.guarded("put shadow tap", || {
                if let Some(body) = req.input.body.take() {
                    req.input.body = Some(shadow::tap_blob(body, db, "bytes_in", true));
                }
            });
        }
        proxied(self.inner.put_object(req)).await
    }

    async fn upload_part(
        &self,
        mut req: S3Request<s3s::dto::UploadPartInput>,
    ) -> S3Result<S3Response<s3s::dto::UploadPartOutput>> {
        self.note("op_upload_part");
        if !bypass_requested() && self.cfg.shadow_chunking {
            let db = Arc::clone(&self.db);
            self.guarded("upload-part shadow tap", || {
                if let Some(body) = req.input.body.take() {
                    req.input.body = Some(shadow::tap_blob(body, db, "bytes_in", true));
                }
            });
        }
        proxied(self.inner.upload_part(req)).await
    }

    async fn get_object(
        &self,
        req: S3Request<s3s::dto::GetObjectInput>,
    ) -> S3Result<S3Response<s3s::dto::GetObjectOutput>> {
        self.note("op_get_object");
        let mut resp = proxied(self.inner.get_object(req)).await?;
        if !bypass_requested() {
            let db = Arc::clone(&self.db);
            self.guarded("get shadow tap", || {
                if let Some(body) = resp.output.body.take() {
                    resp.output.body = Some(shadow::tap_blob(body, db, "bytes_out", false));
                }
            });
        }
        Ok(resp)
    }

    // ---- Everything else: transparent passthrough ------------------------

    async fn head_object(
        &self,
        req: S3Request<s3s::dto::HeadObjectInput>,
    ) -> S3Result<S3Response<s3s::dto::HeadObjectOutput>> {
        self.note("op_head_object");
        proxied(self.inner.head_object(req)).await
    }

    async fn delete_object(
        &self,
        req: S3Request<s3s::dto::DeleteObjectInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteObjectOutput>> {
        self.note("op_delete_object");
        proxied(self.inner.delete_object(req)).await
    }

    async fn create_multipart_upload(
        &self,
        req: S3Request<s3s::dto::CreateMultipartUploadInput>,
    ) -> S3Result<S3Response<s3s::dto::CreateMultipartUploadOutput>> {
        self.note("op_create_multipart_upload");
        proxied(self.inner.create_multipart_upload(req)).await
    }

    async fn complete_multipart_upload(
        &self,
        req: S3Request<s3s::dto::CompleteMultipartUploadInput>,
    ) -> S3Result<S3Response<s3s::dto::CompleteMultipartUploadOutput>> {
        self.note("op_complete_multipart_upload");
        proxied(self.inner.complete_multipart_upload(req)).await
    }

    async fn abort_multipart_upload(
        &self,
        req: S3Request<s3s::dto::AbortMultipartUploadInput>,
    ) -> S3Result<S3Response<s3s::dto::AbortMultipartUploadOutput>> {
        self.note("op_abort_multipart_upload");
        proxied(self.inner.abort_multipart_upload(req)).await
    }

    async fn list_objects_v2(
        &self,
        req: S3Request<s3s::dto::ListObjectsV2Input>,
    ) -> S3Result<S3Response<s3s::dto::ListObjectsV2Output>> {
        self.note("op_list_objects_v2");
        proxied(self.inner.list_objects_v2(req)).await
    }

    async fn copy_object(
        &self,
        req: S3Request<s3s::dto::CopyObjectInput>,
    ) -> S3Result<S3Response<s3s::dto::CopyObjectOutput>> {
        self.note("op_copy_object");
        proxied(self.inner.copy_object(req)).await
    }

    async fn create_bucket(
        &self,
        req: S3Request<s3s::dto::CreateBucketInput>,
    ) -> S3Result<S3Response<s3s::dto::CreateBucketOutput>> {
        self.note("op_create_bucket");
        proxied(self.inner.create_bucket(req)).await
    }

    async fn create_bucket_metadata_configuration(
        &self,
        req: S3Request<s3s::dto::CreateBucketMetadataConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::CreateBucketMetadataConfigurationOutput>> {
        self.note("op_create_bucket_metadata_configuration");
        proxied(self.inner.create_bucket_metadata_configuration(req)).await
    }

    async fn create_bucket_metadata_table_configuration(
        &self,
        req: S3Request<s3s::dto::CreateBucketMetadataTableConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::CreateBucketMetadataTableConfigurationOutput>> {
        self.note("op_create_bucket_metadata_table_configuration");
        proxied(self.inner.create_bucket_metadata_table_configuration(req)).await
    }

    async fn create_session(
        &self,
        req: S3Request<s3s::dto::CreateSessionInput>,
    ) -> S3Result<S3Response<s3s::dto::CreateSessionOutput>> {
        self.note("op_create_session");
        proxied(self.inner.create_session(req)).await
    }

    async fn delete_bucket(
        &self,
        req: S3Request<s3s::dto::DeleteBucketInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketOutput>> {
        self.note("op_delete_bucket");
        proxied(self.inner.delete_bucket(req)).await
    }

    async fn delete_bucket_analytics_configuration(
        &self,
        req: S3Request<s3s::dto::DeleteBucketAnalyticsConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketAnalyticsConfigurationOutput>> {
        self.note("op_delete_bucket_analytics_configuration");
        proxied(self.inner.delete_bucket_analytics_configuration(req)).await
    }

    async fn delete_bucket_cors(
        &self,
        req: S3Request<s3s::dto::DeleteBucketCorsInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketCorsOutput>> {
        self.note("op_delete_bucket_cors");
        proxied(self.inner.delete_bucket_cors(req)).await
    }

    async fn delete_bucket_encryption(
        &self,
        req: S3Request<s3s::dto::DeleteBucketEncryptionInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketEncryptionOutput>> {
        self.note("op_delete_bucket_encryption");
        proxied(self.inner.delete_bucket_encryption(req)).await
    }

    async fn delete_bucket_intelligent_tiering_configuration(
        &self,
        req: S3Request<s3s::dto::DeleteBucketIntelligentTieringConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketIntelligentTieringConfigurationOutput>> {
        self.note("op_delete_bucket_intelligent_tiering_configuration");
        proxied(
            self.inner
                .delete_bucket_intelligent_tiering_configuration(req),
        )
        .await
    }

    async fn delete_bucket_inventory_configuration(
        &self,
        req: S3Request<s3s::dto::DeleteBucketInventoryConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketInventoryConfigurationOutput>> {
        self.note("op_delete_bucket_inventory_configuration");
        proxied(self.inner.delete_bucket_inventory_configuration(req)).await
    }

    async fn delete_bucket_lifecycle(
        &self,
        req: S3Request<s3s::dto::DeleteBucketLifecycleInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketLifecycleOutput>> {
        self.note("op_delete_bucket_lifecycle");
        proxied(self.inner.delete_bucket_lifecycle(req)).await
    }

    async fn delete_bucket_metadata_configuration(
        &self,
        req: S3Request<s3s::dto::DeleteBucketMetadataConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketMetadataConfigurationOutput>> {
        self.note("op_delete_bucket_metadata_configuration");
        proxied(self.inner.delete_bucket_metadata_configuration(req)).await
    }

    async fn delete_bucket_metadata_table_configuration(
        &self,
        req: S3Request<s3s::dto::DeleteBucketMetadataTableConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketMetadataTableConfigurationOutput>> {
        self.note("op_delete_bucket_metadata_table_configuration");
        proxied(self.inner.delete_bucket_metadata_table_configuration(req)).await
    }

    async fn delete_bucket_metrics_configuration(
        &self,
        req: S3Request<s3s::dto::DeleteBucketMetricsConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketMetricsConfigurationOutput>> {
        self.note("op_delete_bucket_metrics_configuration");
        proxied(self.inner.delete_bucket_metrics_configuration(req)).await
    }

    async fn delete_bucket_ownership_controls(
        &self,
        req: S3Request<s3s::dto::DeleteBucketOwnershipControlsInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketOwnershipControlsOutput>> {
        self.note("op_delete_bucket_ownership_controls");
        proxied(self.inner.delete_bucket_ownership_controls(req)).await
    }

    async fn delete_bucket_policy(
        &self,
        req: S3Request<s3s::dto::DeleteBucketPolicyInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketPolicyOutput>> {
        self.note("op_delete_bucket_policy");
        proxied(self.inner.delete_bucket_policy(req)).await
    }

    async fn delete_bucket_replication(
        &self,
        req: S3Request<s3s::dto::DeleteBucketReplicationInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketReplicationOutput>> {
        self.note("op_delete_bucket_replication");
        proxied(self.inner.delete_bucket_replication(req)).await
    }

    async fn delete_bucket_tagging(
        &self,
        req: S3Request<s3s::dto::DeleteBucketTaggingInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketTaggingOutput>> {
        self.note("op_delete_bucket_tagging");
        proxied(self.inner.delete_bucket_tagging(req)).await
    }

    async fn delete_bucket_website(
        &self,
        req: S3Request<s3s::dto::DeleteBucketWebsiteInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteBucketWebsiteOutput>> {
        self.note("op_delete_bucket_website");
        proxied(self.inner.delete_bucket_website(req)).await
    }

    async fn delete_object_tagging(
        &self,
        req: S3Request<s3s::dto::DeleteObjectTaggingInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteObjectTaggingOutput>> {
        self.note("op_delete_object_tagging");
        proxied(self.inner.delete_object_tagging(req)).await
    }

    async fn delete_objects(
        &self,
        req: S3Request<s3s::dto::DeleteObjectsInput>,
    ) -> S3Result<S3Response<s3s::dto::DeleteObjectsOutput>> {
        self.note("op_delete_objects");
        proxied(self.inner.delete_objects(req)).await
    }

    async fn delete_public_access_block(
        &self,
        req: S3Request<s3s::dto::DeletePublicAccessBlockInput>,
    ) -> S3Result<S3Response<s3s::dto::DeletePublicAccessBlockOutput>> {
        self.note("op_delete_public_access_block");
        proxied(self.inner.delete_public_access_block(req)).await
    }

    async fn get_bucket_abac(
        &self,
        req: S3Request<s3s::dto::GetBucketAbacInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketAbacOutput>> {
        self.note("op_get_bucket_abac");
        proxied(self.inner.get_bucket_abac(req)).await
    }

    async fn get_bucket_accelerate_configuration(
        &self,
        req: S3Request<s3s::dto::GetBucketAccelerateConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketAccelerateConfigurationOutput>> {
        self.note("op_get_bucket_accelerate_configuration");
        proxied(self.inner.get_bucket_accelerate_configuration(req)).await
    }

    async fn get_bucket_acl(
        &self,
        req: S3Request<s3s::dto::GetBucketAclInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketAclOutput>> {
        self.note("op_get_bucket_acl");
        proxied(self.inner.get_bucket_acl(req)).await
    }

    async fn get_bucket_analytics_configuration(
        &self,
        req: S3Request<s3s::dto::GetBucketAnalyticsConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketAnalyticsConfigurationOutput>> {
        self.note("op_get_bucket_analytics_configuration");
        proxied(self.inner.get_bucket_analytics_configuration(req)).await
    }

    async fn get_bucket_cors(
        &self,
        req: S3Request<s3s::dto::GetBucketCorsInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketCorsOutput>> {
        self.note("op_get_bucket_cors");
        proxied(self.inner.get_bucket_cors(req)).await
    }

    async fn get_bucket_encryption(
        &self,
        req: S3Request<s3s::dto::GetBucketEncryptionInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketEncryptionOutput>> {
        self.note("op_get_bucket_encryption");
        proxied(self.inner.get_bucket_encryption(req)).await
    }

    async fn get_bucket_intelligent_tiering_configuration(
        &self,
        req: S3Request<s3s::dto::GetBucketIntelligentTieringConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketIntelligentTieringConfigurationOutput>> {
        self.note("op_get_bucket_intelligent_tiering_configuration");
        proxied(self.inner.get_bucket_intelligent_tiering_configuration(req)).await
    }

    async fn get_bucket_inventory_configuration(
        &self,
        req: S3Request<s3s::dto::GetBucketInventoryConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketInventoryConfigurationOutput>> {
        self.note("op_get_bucket_inventory_configuration");
        proxied(self.inner.get_bucket_inventory_configuration(req)).await
    }

    async fn get_bucket_lifecycle_configuration(
        &self,
        req: S3Request<s3s::dto::GetBucketLifecycleConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketLifecycleConfigurationOutput>> {
        self.note("op_get_bucket_lifecycle_configuration");
        proxied(self.inner.get_bucket_lifecycle_configuration(req)).await
    }

    async fn get_bucket_location(
        &self,
        req: S3Request<s3s::dto::GetBucketLocationInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketLocationOutput>> {
        self.note("op_get_bucket_location");
        proxied(self.inner.get_bucket_location(req)).await
    }

    async fn get_bucket_logging(
        &self,
        req: S3Request<s3s::dto::GetBucketLoggingInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketLoggingOutput>> {
        self.note("op_get_bucket_logging");
        proxied(self.inner.get_bucket_logging(req)).await
    }

    async fn get_bucket_metadata_configuration(
        &self,
        req: S3Request<s3s::dto::GetBucketMetadataConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketMetadataConfigurationOutput>> {
        self.note("op_get_bucket_metadata_configuration");
        proxied(self.inner.get_bucket_metadata_configuration(req)).await
    }

    async fn get_bucket_metadata_table_configuration(
        &self,
        req: S3Request<s3s::dto::GetBucketMetadataTableConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketMetadataTableConfigurationOutput>> {
        self.note("op_get_bucket_metadata_table_configuration");
        proxied(self.inner.get_bucket_metadata_table_configuration(req)).await
    }

    async fn get_bucket_metrics_configuration(
        &self,
        req: S3Request<s3s::dto::GetBucketMetricsConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketMetricsConfigurationOutput>> {
        self.note("op_get_bucket_metrics_configuration");
        proxied(self.inner.get_bucket_metrics_configuration(req)).await
    }

    async fn get_bucket_notification_configuration(
        &self,
        req: S3Request<s3s::dto::GetBucketNotificationConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketNotificationConfigurationOutput>> {
        self.note("op_get_bucket_notification_configuration");
        proxied(self.inner.get_bucket_notification_configuration(req)).await
    }

    async fn get_bucket_ownership_controls(
        &self,
        req: S3Request<s3s::dto::GetBucketOwnershipControlsInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketOwnershipControlsOutput>> {
        self.note("op_get_bucket_ownership_controls");
        proxied(self.inner.get_bucket_ownership_controls(req)).await
    }

    async fn get_bucket_policy(
        &self,
        req: S3Request<s3s::dto::GetBucketPolicyInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketPolicyOutput>> {
        self.note("op_get_bucket_policy");
        proxied(self.inner.get_bucket_policy(req)).await
    }

    async fn get_bucket_policy_status(
        &self,
        req: S3Request<s3s::dto::GetBucketPolicyStatusInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketPolicyStatusOutput>> {
        self.note("op_get_bucket_policy_status");
        proxied(self.inner.get_bucket_policy_status(req)).await
    }

    async fn get_bucket_replication(
        &self,
        req: S3Request<s3s::dto::GetBucketReplicationInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketReplicationOutput>> {
        self.note("op_get_bucket_replication");
        proxied(self.inner.get_bucket_replication(req)).await
    }

    async fn get_bucket_request_payment(
        &self,
        req: S3Request<s3s::dto::GetBucketRequestPaymentInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketRequestPaymentOutput>> {
        self.note("op_get_bucket_request_payment");
        proxied(self.inner.get_bucket_request_payment(req)).await
    }

    async fn get_bucket_tagging(
        &self,
        req: S3Request<s3s::dto::GetBucketTaggingInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketTaggingOutput>> {
        self.note("op_get_bucket_tagging");
        proxied(self.inner.get_bucket_tagging(req)).await
    }

    async fn get_bucket_versioning(
        &self,
        req: S3Request<s3s::dto::GetBucketVersioningInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketVersioningOutput>> {
        self.note("op_get_bucket_versioning");
        proxied(self.inner.get_bucket_versioning(req)).await
    }

    async fn get_bucket_website(
        &self,
        req: S3Request<s3s::dto::GetBucketWebsiteInput>,
    ) -> S3Result<S3Response<s3s::dto::GetBucketWebsiteOutput>> {
        self.note("op_get_bucket_website");
        proxied(self.inner.get_bucket_website(req)).await
    }

    async fn get_object_acl(
        &self,
        req: S3Request<s3s::dto::GetObjectAclInput>,
    ) -> S3Result<S3Response<s3s::dto::GetObjectAclOutput>> {
        self.note("op_get_object_acl");
        proxied(self.inner.get_object_acl(req)).await
    }

    async fn get_object_attributes(
        &self,
        req: S3Request<s3s::dto::GetObjectAttributesInput>,
    ) -> S3Result<S3Response<s3s::dto::GetObjectAttributesOutput>> {
        self.note("op_get_object_attributes");
        proxied(self.inner.get_object_attributes(req)).await
    }

    async fn get_object_legal_hold(
        &self,
        req: S3Request<s3s::dto::GetObjectLegalHoldInput>,
    ) -> S3Result<S3Response<s3s::dto::GetObjectLegalHoldOutput>> {
        self.note("op_get_object_legal_hold");
        proxied(self.inner.get_object_legal_hold(req)).await
    }

    async fn get_object_lock_configuration(
        &self,
        req: S3Request<s3s::dto::GetObjectLockConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::GetObjectLockConfigurationOutput>> {
        self.note("op_get_object_lock_configuration");
        proxied(self.inner.get_object_lock_configuration(req)).await
    }

    async fn get_object_retention(
        &self,
        req: S3Request<s3s::dto::GetObjectRetentionInput>,
    ) -> S3Result<S3Response<s3s::dto::GetObjectRetentionOutput>> {
        self.note("op_get_object_retention");
        proxied(self.inner.get_object_retention(req)).await
    }

    async fn get_object_tagging(
        &self,
        req: S3Request<s3s::dto::GetObjectTaggingInput>,
    ) -> S3Result<S3Response<s3s::dto::GetObjectTaggingOutput>> {
        self.note("op_get_object_tagging");
        proxied(self.inner.get_object_tagging(req)).await
    }

    async fn get_object_torrent(
        &self,
        req: S3Request<s3s::dto::GetObjectTorrentInput>,
    ) -> S3Result<S3Response<s3s::dto::GetObjectTorrentOutput>> {
        self.note("op_get_object_torrent");
        proxied(self.inner.get_object_torrent(req)).await
    }

    async fn get_public_access_block(
        &self,
        req: S3Request<s3s::dto::GetPublicAccessBlockInput>,
    ) -> S3Result<S3Response<s3s::dto::GetPublicAccessBlockOutput>> {
        self.note("op_get_public_access_block");
        proxied(self.inner.get_public_access_block(req)).await
    }

    async fn head_bucket(
        &self,
        req: S3Request<s3s::dto::HeadBucketInput>,
    ) -> S3Result<S3Response<s3s::dto::HeadBucketOutput>> {
        self.note("op_head_bucket");
        proxied(self.inner.head_bucket(req)).await
    }

    async fn list_bucket_analytics_configurations(
        &self,
        req: S3Request<s3s::dto::ListBucketAnalyticsConfigurationsInput>,
    ) -> S3Result<S3Response<s3s::dto::ListBucketAnalyticsConfigurationsOutput>> {
        self.note("op_list_bucket_analytics_configurations");
        proxied(self.inner.list_bucket_analytics_configurations(req)).await
    }

    async fn list_bucket_intelligent_tiering_configurations(
        &self,
        req: S3Request<s3s::dto::ListBucketIntelligentTieringConfigurationsInput>,
    ) -> S3Result<S3Response<s3s::dto::ListBucketIntelligentTieringConfigurationsOutput>> {
        self.note("op_list_bucket_intelligent_tiering_configurations");
        proxied(
            self.inner
                .list_bucket_intelligent_tiering_configurations(req),
        )
        .await
    }

    async fn list_bucket_inventory_configurations(
        &self,
        req: S3Request<s3s::dto::ListBucketInventoryConfigurationsInput>,
    ) -> S3Result<S3Response<s3s::dto::ListBucketInventoryConfigurationsOutput>> {
        self.note("op_list_bucket_inventory_configurations");
        proxied(self.inner.list_bucket_inventory_configurations(req)).await
    }

    async fn list_bucket_metrics_configurations(
        &self,
        req: S3Request<s3s::dto::ListBucketMetricsConfigurationsInput>,
    ) -> S3Result<S3Response<s3s::dto::ListBucketMetricsConfigurationsOutput>> {
        self.note("op_list_bucket_metrics_configurations");
        proxied(self.inner.list_bucket_metrics_configurations(req)).await
    }

    async fn list_buckets(
        &self,
        req: S3Request<s3s::dto::ListBucketsInput>,
    ) -> S3Result<S3Response<s3s::dto::ListBucketsOutput>> {
        self.note("op_list_buckets");
        proxied(self.inner.list_buckets(req)).await
    }

    async fn list_directory_buckets(
        &self,
        req: S3Request<s3s::dto::ListDirectoryBucketsInput>,
    ) -> S3Result<S3Response<s3s::dto::ListDirectoryBucketsOutput>> {
        self.note("op_list_directory_buckets");
        proxied(self.inner.list_directory_buckets(req)).await
    }

    async fn list_multipart_uploads(
        &self,
        req: S3Request<s3s::dto::ListMultipartUploadsInput>,
    ) -> S3Result<S3Response<s3s::dto::ListMultipartUploadsOutput>> {
        self.note("op_list_multipart_uploads");
        proxied(self.inner.list_multipart_uploads(req)).await
    }

    async fn list_object_versions(
        &self,
        req: S3Request<s3s::dto::ListObjectVersionsInput>,
    ) -> S3Result<S3Response<s3s::dto::ListObjectVersionsOutput>> {
        self.note("op_list_object_versions");
        proxied(self.inner.list_object_versions(req)).await
    }

    async fn list_objects(
        &self,
        req: S3Request<s3s::dto::ListObjectsInput>,
    ) -> S3Result<S3Response<s3s::dto::ListObjectsOutput>> {
        self.note("op_list_objects");
        proxied(self.inner.list_objects(req)).await
    }

    async fn list_parts(
        &self,
        req: S3Request<s3s::dto::ListPartsInput>,
    ) -> S3Result<S3Response<s3s::dto::ListPartsOutput>> {
        self.note("op_list_parts");
        proxied(self.inner.list_parts(req)).await
    }

    async fn put_bucket_abac(
        &self,
        req: S3Request<s3s::dto::PutBucketAbacInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketAbacOutput>> {
        self.note("op_put_bucket_abac");
        proxied(self.inner.put_bucket_abac(req)).await
    }

    async fn put_bucket_accelerate_configuration(
        &self,
        req: S3Request<s3s::dto::PutBucketAccelerateConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketAccelerateConfigurationOutput>> {
        self.note("op_put_bucket_accelerate_configuration");
        proxied(self.inner.put_bucket_accelerate_configuration(req)).await
    }

    async fn put_bucket_acl(
        &self,
        req: S3Request<s3s::dto::PutBucketAclInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketAclOutput>> {
        self.note("op_put_bucket_acl");
        proxied(self.inner.put_bucket_acl(req)).await
    }

    async fn put_bucket_analytics_configuration(
        &self,
        req: S3Request<s3s::dto::PutBucketAnalyticsConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketAnalyticsConfigurationOutput>> {
        self.note("op_put_bucket_analytics_configuration");
        proxied(self.inner.put_bucket_analytics_configuration(req)).await
    }

    async fn put_bucket_cors(
        &self,
        req: S3Request<s3s::dto::PutBucketCorsInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketCorsOutput>> {
        self.note("op_put_bucket_cors");
        proxied(self.inner.put_bucket_cors(req)).await
    }

    async fn put_bucket_encryption(
        &self,
        req: S3Request<s3s::dto::PutBucketEncryptionInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketEncryptionOutput>> {
        self.note("op_put_bucket_encryption");
        proxied(self.inner.put_bucket_encryption(req)).await
    }

    async fn put_bucket_intelligent_tiering_configuration(
        &self,
        req: S3Request<s3s::dto::PutBucketIntelligentTieringConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketIntelligentTieringConfigurationOutput>> {
        self.note("op_put_bucket_intelligent_tiering_configuration");
        proxied(self.inner.put_bucket_intelligent_tiering_configuration(req)).await
    }

    async fn put_bucket_inventory_configuration(
        &self,
        req: S3Request<s3s::dto::PutBucketInventoryConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketInventoryConfigurationOutput>> {
        self.note("op_put_bucket_inventory_configuration");
        proxied(self.inner.put_bucket_inventory_configuration(req)).await
    }

    async fn put_bucket_lifecycle_configuration(
        &self,
        req: S3Request<s3s::dto::PutBucketLifecycleConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketLifecycleConfigurationOutput>> {
        self.note("op_put_bucket_lifecycle_configuration");
        proxied(self.inner.put_bucket_lifecycle_configuration(req)).await
    }

    async fn put_bucket_logging(
        &self,
        req: S3Request<s3s::dto::PutBucketLoggingInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketLoggingOutput>> {
        self.note("op_put_bucket_logging");
        proxied(self.inner.put_bucket_logging(req)).await
    }

    async fn put_bucket_metrics_configuration(
        &self,
        req: S3Request<s3s::dto::PutBucketMetricsConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketMetricsConfigurationOutput>> {
        self.note("op_put_bucket_metrics_configuration");
        proxied(self.inner.put_bucket_metrics_configuration(req)).await
    }

    async fn put_bucket_notification_configuration(
        &self,
        req: S3Request<s3s::dto::PutBucketNotificationConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketNotificationConfigurationOutput>> {
        self.note("op_put_bucket_notification_configuration");
        proxied(self.inner.put_bucket_notification_configuration(req)).await
    }

    async fn put_bucket_ownership_controls(
        &self,
        req: S3Request<s3s::dto::PutBucketOwnershipControlsInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketOwnershipControlsOutput>> {
        self.note("op_put_bucket_ownership_controls");
        proxied(self.inner.put_bucket_ownership_controls(req)).await
    }

    async fn put_bucket_policy(
        &self,
        req: S3Request<s3s::dto::PutBucketPolicyInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketPolicyOutput>> {
        self.note("op_put_bucket_policy");
        proxied(self.inner.put_bucket_policy(req)).await
    }

    async fn put_bucket_replication(
        &self,
        req: S3Request<s3s::dto::PutBucketReplicationInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketReplicationOutput>> {
        self.note("op_put_bucket_replication");
        proxied(self.inner.put_bucket_replication(req)).await
    }

    async fn put_bucket_request_payment(
        &self,
        req: S3Request<s3s::dto::PutBucketRequestPaymentInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketRequestPaymentOutput>> {
        self.note("op_put_bucket_request_payment");
        proxied(self.inner.put_bucket_request_payment(req)).await
    }

    async fn put_bucket_tagging(
        &self,
        req: S3Request<s3s::dto::PutBucketTaggingInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketTaggingOutput>> {
        self.note("op_put_bucket_tagging");
        proxied(self.inner.put_bucket_tagging(req)).await
    }

    async fn put_bucket_versioning(
        &self,
        req: S3Request<s3s::dto::PutBucketVersioningInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketVersioningOutput>> {
        self.note("op_put_bucket_versioning");
        proxied(self.inner.put_bucket_versioning(req)).await
    }

    async fn put_bucket_website(
        &self,
        req: S3Request<s3s::dto::PutBucketWebsiteInput>,
    ) -> S3Result<S3Response<s3s::dto::PutBucketWebsiteOutput>> {
        self.note("op_put_bucket_website");
        proxied(self.inner.put_bucket_website(req)).await
    }

    async fn put_object_acl(
        &self,
        req: S3Request<s3s::dto::PutObjectAclInput>,
    ) -> S3Result<S3Response<s3s::dto::PutObjectAclOutput>> {
        self.note("op_put_object_acl");
        proxied(self.inner.put_object_acl(req)).await
    }

    async fn put_object_legal_hold(
        &self,
        req: S3Request<s3s::dto::PutObjectLegalHoldInput>,
    ) -> S3Result<S3Response<s3s::dto::PutObjectLegalHoldOutput>> {
        self.note("op_put_object_legal_hold");
        proxied(self.inner.put_object_legal_hold(req)).await
    }

    async fn put_object_lock_configuration(
        &self,
        req: S3Request<s3s::dto::PutObjectLockConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::PutObjectLockConfigurationOutput>> {
        self.note("op_put_object_lock_configuration");
        proxied(self.inner.put_object_lock_configuration(req)).await
    }

    async fn put_object_retention(
        &self,
        req: S3Request<s3s::dto::PutObjectRetentionInput>,
    ) -> S3Result<S3Response<s3s::dto::PutObjectRetentionOutput>> {
        self.note("op_put_object_retention");
        proxied(self.inner.put_object_retention(req)).await
    }

    async fn put_object_tagging(
        &self,
        req: S3Request<s3s::dto::PutObjectTaggingInput>,
    ) -> S3Result<S3Response<s3s::dto::PutObjectTaggingOutput>> {
        self.note("op_put_object_tagging");
        proxied(self.inner.put_object_tagging(req)).await
    }

    async fn put_public_access_block(
        &self,
        req: S3Request<s3s::dto::PutPublicAccessBlockInput>,
    ) -> S3Result<S3Response<s3s::dto::PutPublicAccessBlockOutput>> {
        self.note("op_put_public_access_block");
        proxied(self.inner.put_public_access_block(req)).await
    }

    async fn rename_object(
        &self,
        req: S3Request<s3s::dto::RenameObjectInput>,
    ) -> S3Result<S3Response<s3s::dto::RenameObjectOutput>> {
        self.note("op_rename_object");
        proxied(self.inner.rename_object(req)).await
    }

    async fn restore_object(
        &self,
        req: S3Request<s3s::dto::RestoreObjectInput>,
    ) -> S3Result<S3Response<s3s::dto::RestoreObjectOutput>> {
        self.note("op_restore_object");
        proxied(self.inner.restore_object(req)).await
    }

    async fn select_object_content(
        &self,
        req: S3Request<s3s::dto::SelectObjectContentInput>,
    ) -> S3Result<S3Response<s3s::dto::SelectObjectContentOutput>> {
        self.note("op_select_object_content");
        proxied(self.inner.select_object_content(req)).await
    }

    async fn update_bucket_metadata_annotation_table_configuration(
        &self,
        req: S3Request<s3s::dto::UpdateBucketMetadataAnnotationTableConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::UpdateBucketMetadataAnnotationTableConfigurationOutput>>
    {
        self.note("op_update_bucket_metadata_annotation_table_configuration");
        proxied(
            self.inner
                .update_bucket_metadata_annotation_table_configuration(req),
        )
        .await
    }

    async fn update_bucket_metadata_inventory_table_configuration(
        &self,
        req: S3Request<s3s::dto::UpdateBucketMetadataInventoryTableConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::UpdateBucketMetadataInventoryTableConfigurationOutput>> {
        self.note("op_update_bucket_metadata_inventory_table_configuration");
        proxied(
            self.inner
                .update_bucket_metadata_inventory_table_configuration(req),
        )
        .await
    }

    async fn update_bucket_metadata_journal_table_configuration(
        &self,
        req: S3Request<s3s::dto::UpdateBucketMetadataJournalTableConfigurationInput>,
    ) -> S3Result<S3Response<s3s::dto::UpdateBucketMetadataJournalTableConfigurationOutput>> {
        self.note("op_update_bucket_metadata_journal_table_configuration");
        proxied(
            self.inner
                .update_bucket_metadata_journal_table_configuration(req),
        )
        .await
    }

    async fn upload_part_copy(
        &self,
        req: S3Request<s3s::dto::UploadPartCopyInput>,
    ) -> S3Result<S3Response<s3s::dto::UploadPartCopyOutput>> {
        self.note("op_upload_part_copy");
        proxied(self.inner.upload_part_copy(req)).await
    }

    async fn write_get_object_response(
        &self,
        req: S3Request<s3s::dto::WriteGetObjectResponseInput>,
    ) -> S3Result<S3Response<s3s::dto::WriteGetObjectResponseOutput>> {
        self.note("op_write_get_object_response");
        proxied(self.inner.write_get_object_response(req)).await
    }
}
