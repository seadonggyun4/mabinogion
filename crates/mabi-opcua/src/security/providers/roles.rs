use std::sync::Arc;

use super::RoleMapper;
use crate::security::manager::{RoleMappingRule, SecurityManagerConfig};

pub(crate) struct StaticRoleMapper;

impl RoleMapper for StaticRoleMapper {
    fn map_roles(&self, mut roles: Vec<String>) -> Vec<String> {
        roles.sort();
        roles.dedup();
        roles
    }
}

pub(crate) struct ConfigDrivenRoleMapper {
    rules: Vec<RoleMappingRule>,
}

impl ConfigDrivenRoleMapper {
    pub(crate) fn new(rules: Vec<RoleMappingRule>) -> Self {
        Self { rules }
    }
}

impl RoleMapper for ConfigDrivenRoleMapper {
    fn map_roles(&self, mut roles: Vec<String>) -> Vec<String> {
        let mut additions = Vec::new();
        for rule in &self.rules {
            if roles.iter().any(|role| role == &rule.match_role) {
                additions.extend(rule.add_roles.clone());
            }
        }
        roles.extend(additions);
        roles.sort();
        roles.dedup();
        roles
    }
}

pub(crate) fn build_role_mapper(config: &SecurityManagerConfig) -> Arc<dyn RoleMapper> {
    if config.role_rules.is_empty() {
        Arc::new(StaticRoleMapper)
    } else {
        Arc::new(ConfigDrivenRoleMapper::new(config.role_rules.clone()))
    }
}
