use super::*;
go_struct! {
    pub struct QuotaMetric("pluginapi.QuotaMetric") {
        "key" => key: String,
        "label" => label: String,
        "value" => value: f64,
        "unit" omitempty => unit: String,
        "format" omitempty => format: String,
        "currency" omitempty => currency: String,
    }
}

// Quota responses carry custom `UnmarshalJSON` methods that also accept snake_case keys.
// Go keeps the camelCase value unless it is the zero value and the snake_case one is not.

fn alt<T: PartialEq + Default>(camel: T, snake: Option<T>) -> T {
    match snake {
        Some(snake) if camel == T::default() && snake != T::default() => snake,
        _ => camel,
    }
}

go_struct! {
    pub struct QuotaSubscription("pluginapi.QuotaSubscription") {
        "plan" omitempty => plan: String,
        "tierName" omitempty => tier_name: String,
        "tierId" omitempty => tier_id: String,
    }
}

go_struct! {
    pub struct QuotaSubscriptionAlt("pluginapi.QuotaSubscription") {
        "tier_name" => tier_name: Option<String>,
        "tier_id" => tier_id: Option<String>,
    }
}

go_struct! {
    pub struct QuotaBucket("pluginapi.QuotaBucket") {
        "window" omitempty => window: String,
        "remainingFraction" => remaining_fraction: f64,
        "resetTime" omitempty => reset_time: String,
        "description" omitempty => description: String,
    }
}

go_struct! {
    pub struct QuotaBucketAlt("pluginapi.QuotaBucket") {
        "remainingFraction" => remaining_fraction: Option<f64>,
        "remaining_fraction" => alt_remaining_fraction: Option<f64>,
        "reset_time" => reset_time: Option<String>,
    }
}

go_struct! {
    pub struct QuotaGroup("pluginapi.QuotaGroup") {
        "displayName" omitempty => display_name: String,
        "buckets" omitempty => buckets: Vec<QuotaBucket>,
    }
}

go_struct! {
    pub struct QuotaGroupAlt("pluginapi.QuotaGroup") {
        "display_name" => display_name: Option<String>,
    }
}

go_struct! {
    pub struct QuotaFetchResponse("pluginapi.QuotaFetchResponse") {
        "subscription" omitempty => subscription: Option<QuotaSubscription>,
        "summary" omitempty => summary: Vec<QuotaMetric>,
        "serverTimeOffsetMs" omitempty => server_time_offset_ms: i64,
        "groups" omitempty => groups: Vec<QuotaGroup>,
    }
}

go_struct! {
    pub struct QuotaFetchResponseAlt("pluginapi.QuotaFetchResponse") {
        "server_time_offset_ms" => server_time_offset_ms: Option<i64>,
    }
}

/// Applies the snake_case fallbacks of the quota `UnmarshalJSON` methods to a decoded
/// `quota.fetch` result. `raw` is the result object the plugin returned.
pub fn quota_fetch_from_value(raw: &Node) -> Result<QuotaFetchResponse, crate::gojson::DecodeError> {
    let mut out = QuotaFetchResponse::decode(raw)?;
    let top: QuotaFetchResponseAlt = crate::gojson::decode_struct(raw)?;
    out.server_time_offset_ms = alt(out.server_time_offset_ms, top.server_time_offset_ms);
    if let (Some(sub), Some(raw_sub)) = (out.subscription.as_mut(), raw.member("subscription")) {
        let a: QuotaSubscriptionAlt = crate::gojson::decode_struct(raw_sub)?;
        sub.tier_name = alt(std::mem::take(&mut sub.tier_name), a.tier_name);
        sub.tier_id = alt(std::mem::take(&mut sub.tier_id), a.tier_id);
    }
    if let Some(Node::Array(groups)) = raw.member("groups") {
        for (group, raw_group) in out.groups.iter_mut().zip(groups.iter()) {
            let a: QuotaGroupAlt = crate::gojson::decode_struct(raw_group)?;
            group.display_name = alt(std::mem::take(&mut group.display_name), a.display_name);
            if let Some(Node::Array(buckets)) = raw_group.member("buckets") {
                for (bucket, raw_bucket) in group.buckets.iter_mut().zip(buckets.iter()) {
                    let a: QuotaBucketAlt = crate::gojson::decode_struct(raw_bucket)?;
                    // A present camelCase pointer wins even when zero.
                    bucket.remaining_fraction = a.remaining_fraction.or(a.alt_remaining_fraction).unwrap_or_default();
                    bucket.reset_time = alt(std::mem::take(&mut bucket.reset_time), a.reset_time);
                }
            }
        }
    }
    Ok(out)
}
