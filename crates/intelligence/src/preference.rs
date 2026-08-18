use rill_model_api::ModelIdentity;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{IntelligenceError, IntelligenceService, decode_vector, unix_now};

const MODEL_VERSION: i64 = 1;
const FEATURE_VERSION: i64 = 1;
const EMBEDDING_FEATURES: usize = 32;
const FEATURE_COUNT: usize = EMBEDDING_FEATURES + 3;
const COEFFICIENT_COUNT: usize = FEATURE_COUNT + 1;
type StoredPreferenceModel = (i64, i64, String, String, String, String, i64, i64);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreferenceRefitPayload {
    pub user_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PreferenceModel {
    coefficients: Vec<f32>,
}

impl PreferenceModel {
    pub(crate) fn predict(
        &self,
        embedding: &[f32],
        published_at: Option<i64>,
        coverage: u32,
        affinity: f32,
    ) -> Option<f32> {
        let features = feature_vector(embedding, published_at, coverage, affinity)?;
        let score = self.coefficients[0]
            + self.coefficients[1..]
                .iter()
                .zip(features)
                .map(|(weight, feature)| weight * feature)
                .sum::<f32>();
        Some(sigmoid(score))
    }
}

impl IntelligenceService {
    pub(crate) fn preference_model(
        &self,
        user_id: &str,
        identity: &ModelIdentity,
    ) -> Result<Option<PreferenceModel>, IntelligenceError> {
        let raw: Option<StoredPreferenceModel> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT model_version, feature_version, embedding_provider,
                         embedding_model, embedding_version, coefficients_json,
                         positive_count, negative_count FROM preference_models WHERE user_id=?1",
                    [user_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                        ))
                    },
                )
                .optional()
        })?;
        let Some((model, feature, provider, embedding_model, version, raw, positive, negative)) =
            raw
        else {
            return Ok(None);
        };
        if model != MODEL_VERSION
            || feature != FEATURE_VERSION
            || provider != identity.provider
            || embedding_model != identity.model
            || version != identity.version
            || positive == 0
            || negative == 0
        {
            return Ok(None);
        }
        let Ok(coefficients) = serde_json::from_str::<Vec<f32>>(&raw) else {
            return Ok(None);
        };
        if coefficients.len() != COEFFICIENT_COUNT
            || coefficients.iter().any(|value| !value.is_finite())
        {
            return Ok(None);
        }
        Ok(Some(PreferenceModel { coefficients }))
    }

    pub fn refit_preference_model(&self, user_id: &str) -> Result<bool, IntelligenceError> {
        let identity = self.embedding.identity();
        let labeled_count = self.labeled_event_count(user_id)?;
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT fe.feedback, er.vector_f32le, d.published_at,
             (SELECT count(*) FROM story_memberships sm WHERE sm.story_id=fe.story_id),
             d.publisher,
             coalesce((SELECT group_concat(DISTINCT dc.source_instance_id)
               FROM document_curators dc WHERE dc.document_id=d.id), ''),
             coalesce((SELECT group_concat(DISTINCT dc.curator_id)
               FROM document_curators dc WHERE dc.document_id=d.id), '')
             FROM feedback_events fe JOIN documents d ON d.id=fe.document_id
             JOIN embedding_records er ON er.id=(SELECT latest.id FROM embedding_records latest
               WHERE latest.entity_type='document' AND latest.entity_id=d.id
               AND latest.provider=?2 AND latest.model=?3 AND latest.model_version=?4
               ORDER BY latest.created_at DESC LIMIT 1)
             WHERE fe.user_id=?1 AND fe.feedback IN ('like', 'dislike')
             ORDER BY fe.created_at DESC, fe.rowid DESC LIMIT ?5",
        )?;
        let rows = statement.query_map(
            params![
                user_id,
                identity.provider,
                identity.model,
                identity.version,
                i64::try_from(self.preference_fit_window).unwrap_or(i64::MAX)
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )?;
        let rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        drop(connection);
        let mut samples = Vec::new();
        for row in rows {
            let (feedback, bytes, published_at, coverage, publisher, sources, curators) = row;
            let Some(embedding) = decode_vector(&bytes) else {
                continue;
            };
            let affinity = crate::streams::affinity_score(
                &self.pool,
                user_id,
                publisher.as_deref(),
                &split_list(sources),
                &split_list(curators),
            )?;
            if let Some(features) = feature_vector(&embedding, published_at, coverage, affinity) {
                samples.push((features, u8::from(feedback == "like")));
            }
        }
        let positive_count = samples.iter().filter(|(_, label)| *label == 1).count();
        let negative_count = samples.len().saturating_sub(positive_count);
        if positive_count == 0 || negative_count == 0 {
            return Ok(false);
        }
        let coefficients = fit(&samples);
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO preference_models(user_id, model_version, feature_version,
             embedding_provider, embedding_model, embedding_version, coefficients_json,
             sample_count, positive_count, negative_count, trained_event_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(user_id) DO UPDATE SET model_version=excluded.model_version,
             feature_version=excluded.feature_version,
             embedding_provider=excluded.embedding_provider,
             embedding_model=excluded.embedding_model,
             embedding_version=excluded.embedding_version,
             coefficients_json=excluded.coefficients_json, sample_count=excluded.sample_count,
             positive_count=excluded.positive_count, negative_count=excluded.negative_count,
             trained_event_count=excluded.trained_event_count, updated_at=unixepoch()",
            params![
                user_id,
                MODEL_VERSION,
                FEATURE_VERSION,
                identity.provider,
                identity.model,
                identity.version,
                serde_json::to_string(&coefficients)?,
                i64::try_from(samples.len()).unwrap_or(i64::MAX),
                i64::try_from(positive_count).unwrap_or(i64::MAX),
                i64::try_from(negative_count).unwrap_or(i64::MAX),
                labeled_count,
            ],
        )?;
        transaction.execute(
            "DELETE FROM recommendation_runs WHERE user_id=?1",
            [user_id],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    pub(crate) fn enqueue_preference_refit_if_due(
        &self,
        user_id: &str,
    ) -> Result<(), IntelligenceError> {
        let (labeled, trained) = self.pool.with_connection(|connection| {
            Ok::<_, rusqlite::Error>((
                connection.query_row(
                    "SELECT count(*) FROM feedback_events
                     WHERE user_id=?1 AND feedback IN ('like', 'dislike')",
                    [user_id],
                    |row| row.get::<_, i64>(0),
                )?,
                connection
                    .query_row(
                        "SELECT trained_event_count FROM preference_models WHERE user_id=?1",
                        [user_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?
                    .unwrap_or(0),
            ))
        })?;
        if labeled.saturating_sub(trained)
            < i64::try_from(self.preference_refit_batch_size).unwrap_or(i64::MAX)
        {
            return Ok(());
        }
        self.jobs.enqueue_coalesced_queued(
            rill_jobs::JobKind::RefitPreferenceModel,
            &serde_json::to_value(PreferenceRefitPayload {
                user_id: user_id.to_owned(),
            })?,
            rill_jobs::EnqueueOptions {
                visibility_scope: Some(format!("user:{user_id}")),
                priority: -3,
                max_attempts: 3,
                ..Default::default()
            },
        )?;
        Ok(())
    }

    pub(crate) fn enqueue_preference_refits_for_document(&self, document_id: &str) {
        let users = self.pool.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT DISTINCT user_id FROM feedback_events WHERE document_id=?1
                 AND feedback IN ('like', 'dislike')",
            )?;
            let rows = statement.query_map([document_id], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        });
        match users {
            Ok(users) => {
                for user_id in users {
                    if let Err(error) = self.enqueue_preference_refit_if_due(&user_id) {
                        warn!(error = %error, %user_id, "preference refit could not be queued");
                    }
                }
            }
            Err(error) => warn!(error = %error, %document_id, "feedback users could not be loaded"),
        }
    }

    fn labeled_event_count(&self, user_id: &str) -> Result<i64, IntelligenceError> {
        Ok(self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT count(*) FROM feedback_events
                 WHERE user_id=?1 AND feedback IN ('like', 'dislike')",
                [user_id],
                |row| row.get(0),
            )
        })?)
    }
}

fn feature_vector(
    embedding: &[f32],
    published_at: Option<i64>,
    coverage: u32,
    affinity: f32,
) -> Option<[f32; FEATURE_COUNT]> {
    if embedding.is_empty() || embedding.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let mut output = [0.0; FEATURE_COUNT];
    for (index, value) in embedding.iter().enumerate() {
        output[index % EMBEDDING_FEATURES] += value;
    }
    let norm = output[..EMBEDDING_FEATURES]
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if norm > f32::EPSILON {
        for value in &mut output[..EMBEDDING_FEATURES] {
            *value /= norm;
        }
    }
    let age_hours = published_at.map_or(72.0, |published| {
        unix_now().saturating_sub(published).max(0) as f32 / 3600.0
    });
    output[EMBEDDING_FEATURES] = 1.0 / (1.0 + age_hours / 72.0);
    output[EMBEDDING_FEATURES + 1] = (coverage.max(1) as f32).ln_1p().min(4.0) / 4.0;
    output[EMBEDDING_FEATURES + 2] = affinity.clamp(-0.5, 0.5) + 0.5;
    Some(output)
}

fn fit(samples: &[([f32; FEATURE_COUNT], u8)]) -> Vec<f32> {
    let mut coefficients = vec![0.0_f32; COEFFICIENT_COUNT];
    let scale = 1.0 / samples.len() as f32;
    for _ in 0..200 {
        let mut gradient = [0.0_f32; COEFFICIENT_COUNT];
        for (features, label) in samples {
            let score = coefficients[0]
                + coefficients[1..]
                    .iter()
                    .zip(features)
                    .map(|(weight, feature)| weight * feature)
                    .sum::<f32>();
            let error = sigmoid(score) - f32::from(*label);
            gradient[0] += error;
            for (gradient, feature) in gradient[1..].iter_mut().zip(features) {
                *gradient += error * feature;
            }
        }
        coefficients[0] -= 0.2 * gradient[0] * scale;
        for (weight, gradient) in coefficients[1..].iter_mut().zip(&gradient[1..]) {
            *weight -= 0.2 * (gradient * scale + 0.01 * *weight);
        }
    }
    coefficients
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value.clamp(-30.0, 30.0)).exp())
}

fn split_list(value: String) -> Vec<String> {
    value
        .split(',')
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logistic_fit_separates_two_classes() {
        let mut negative = [0.0; FEATURE_COUNT];
        negative[0] = -1.0;
        let mut positive = [0.0; FEATURE_COUNT];
        positive[0] = 1.0;
        let coefficients = fit(&[(negative, 0), (positive, 1)]);
        let model = PreferenceModel { coefficients };
        assert!(model.predict(&[1.0], None, 1, 0.0).unwrap() > 0.5);
        assert!(model.predict(&[-1.0], None, 1, 0.0).unwrap() < 0.5);
    }
}
