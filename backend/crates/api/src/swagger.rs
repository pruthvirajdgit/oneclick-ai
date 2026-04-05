//! OpenAPI / Swagger UI configuration.
//!
//! Generates an OpenAPI spec and serves a Swagger UI page at
//! `/swagger-ui/`. The spec JSON is available at `/api-docs/openapi.json`.
//! Paths are defined manually to avoid coupling route handlers to utoipa macros.

use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::state::AppState;

/// Return a router serving Swagger UI and the OpenAPI JSON spec.
pub fn swagger_routes() -> Router<AppState> {
    Router::new()
        .route("/api-docs/openapi.json", get(openapi_json))
        .route("/swagger-ui", get(swagger_ui_html))
        .route("/swagger-ui/", get(swagger_ui_html))
}

/// Serve the OpenAPI spec as JSON.
async fn openapi_json() -> impl IntoResponse {
    Json(openapi_spec())
}

fn openapi_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "OneClick.ai API",
            "version": "0.1.0",
            "description": "Multi-tenant AI agent platform — manage agents, schedules, and usage."
        },
        "servers": [{"url": "http://localhost:8080"}],
        "components": {
            "securitySchemes": {
                "bearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "JWT"
                }
            }
        },
        "tags": [
            {"name": "auth", "description": "Signup, login, token refresh"},
            {"name": "agents", "description": "Agent CRUD + orchestrator"},
            {"name": "schedules", "description": "Cron schedule management"},
            {"name": "usage", "description": "Token usage statistics"},
            {"name": "notifications", "description": "User notifications"},
            {"name": "system", "description": "Health and metrics"}
        ],
        "paths": {
            "/api/auth/signup": {
                "post": {
                    "tags": ["auth"],
                    "summary": "Create account",
                    "operationId": "signup",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["email", "password"],
                                    "properties": {
                                        "email": {"type": "string", "format": "email", "example": "demo@oneclick.ai"},
                                        "password": {"type": "string", "minLength": 8, "example": "MyPass123!"}
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "201": {"description": "Account created, JWT returned"},
                        "409": {"description": "Email already registered"}
                    }
                }
            },
            "/api/auth/login": {
                "post": {
                    "tags": ["auth"],
                    "summary": "Login and get JWT",
                    "operationId": "login",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["email", "password"],
                                    "properties": {
                                        "email": {"type": "string", "format": "email", "example": "demo@oneclick.ai"},
                                        "password": {"type": "string", "example": "MyPass123!"}
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {"description": "JWT token + user info"},
                        "401": {"description": "Invalid credentials"}
                    }
                }
            },
            "/api/auth/refresh": {
                "post": {
                    "tags": ["auth"],
                    "summary": "Refresh JWT token",
                    "operationId": "refreshToken",
                    "security": [{"bearerAuth": []}],
                    "responses": {
                        "200": {"description": "New JWT token"},
                        "401": {"description": "Invalid or expired token"}
                    }
                }
            },
            "/api/agents": {
                "get": {
                    "tags": ["agents"],
                    "summary": "List your agents",
                    "operationId": "listAgents",
                    "security": [{"bearerAuth": []}],
                    "responses": {
                        "200": {"description": "Array of agents"}
                    }
                },
                "post": {
                    "tags": ["agents"],
                    "summary": "Create a new agent",
                    "operationId": "createAgent",
                    "security": [{"bearerAuth": []}],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["model"],
                                    "properties": {
                                        "model": {"type": "string", "example": "groq/llama-3.3-70b"}
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "201": {"description": "Agent created"},
                        "403": {"description": "Capacity reached"}
                    }
                }
            },
            "/api/agents/{id}": {
                "get": {
                    "tags": ["agents"],
                    "summary": "Get agent details",
                    "operationId": "getAgent",
                    "security": [{"bearerAuth": []}],
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                    "responses": {
                        "200": {"description": "Agent details"},
                        "404": {"description": "Not found"}
                    }
                },
                "delete": {
                    "tags": ["agents"],
                    "summary": "Destroy agent",
                    "operationId": "deleteAgent",
                    "security": [{"bearerAuth": []}],
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                    "responses": {
                        "204": {"description": "Agent destroyed"},
                        "404": {"description": "Not found"}
                    }
                }
            },
            "/api/schedules": {
                "get": {
                    "tags": ["schedules"],
                    "summary": "List your schedules",
                    "operationId": "listSchedules",
                    "security": [{"bearerAuth": []}],
                    "responses": {
                        "200": {"description": "Array of scheduled jobs"}
                    }
                },
                "post": {
                    "tags": ["schedules"],
                    "summary": "Create a scheduled job",
                    "operationId": "createSchedule",
                    "security": [{"bearerAuth": []}],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["agent_id", "cron_expr", "task_message"],
                                    "properties": {
                                        "agent_id": {"type": "string", "format": "uuid"},
                                        "cron_expr": {"type": "string", "example": "0 */3 * * *"},
                                        "task_message": {"type": "string", "example": "Check flight prices to Bangalore"}
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "201": {"description": "Schedule created"},
                        "400": {"description": "Invalid cron expression"}
                    }
                }
            },
            "/api/schedules/{id}": {
                "delete": {
                    "tags": ["schedules"],
                    "summary": "Cancel a schedule",
                    "operationId": "deleteSchedule",
                    "security": [{"bearerAuth": []}],
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                    "responses": {
                        "204": {"description": "Schedule cancelled"}
                    }
                }
            },
            "/api/usage": {
                "get": {
                    "tags": ["usage"],
                    "summary": "Get usage statistics",
                    "operationId": "getUsage",
                    "security": [{"bearerAuth": []}],
                    "responses": {
                        "200": {"description": "Today + all-time usage stats"}
                    }
                }
            },
            "/api/notifications": {
                "get": {
                    "tags": ["notifications"],
                    "summary": "List notifications",
                    "operationId": "listNotifications",
                    "security": [{"bearerAuth": []}],
                    "parameters": [
                        {"name": "limit", "in": "query", "schema": {"type": "integer", "default": 20}},
                        {"name": "offset", "in": "query", "schema": {"type": "integer", "default": 0}}
                    ],
                    "responses": {
                        "200": {"description": "Array of notifications"}
                    }
                }
            },
            "/api/notifications/{id}/read": {
                "post": {
                    "tags": ["notifications"],
                    "summary": "Mark notification as read",
                    "operationId": "markNotificationRead",
                    "security": [{"bearerAuth": []}],
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "integer"}}],
                    "responses": {
                        "200": {"description": "Marked as read"},
                        "404": {"description": "Not found"}
                    }
                }
            },
            "/health": {
                "get": {
                    "tags": ["system"],
                    "summary": "Liveness probe",
                    "operationId": "healthCheck",
                    "responses": {
                        "200": {"description": "\"ok\""}
                    }
                }
            },
            "/metrics": {
                "get": {
                    "tags": ["system"],
                    "summary": "Prometheus metrics",
                    "operationId": "metrics",
                    "responses": {
                        "200": {"description": "Prometheus text format"}
                    }
                }
            }
        }
    })
}

/// Serve a Swagger UI HTML page that loads from the official CDN.
async fn swagger_ui_html() -> impl IntoResponse {
    Html(SWAGGER_UI_HTML)
}

/// Embedded Swagger UI HTML using pinned CDN versions.
///
/// Note: In production, consider serving these assets from the backend image
/// to eliminate CDN dependency. SRI hashes would require updating on each
/// version bump, so pinning to exact versions provides a reasonable balance.
const SWAGGER_UI_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>OneClick.ai — API Documentation</title>
    <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5.18.2/swagger-ui.css">
</head>
<body>
    <div id="swagger-ui"></div>
    <script src="https://unpkg.com/swagger-ui-dist@5.18.2/swagger-ui-bundle.js"></script>
    <script src="https://unpkg.com/swagger-ui-dist@5.18.2/swagger-ui-standalone-preset.js"></script>
    <script>
        SwaggerUIBundle({
            url: "/api-docs/openapi.json",
            dom_id: "#swagger-ui",
            presets: [SwaggerUIBundle.presets.apis, SwaggerUIStandalonePreset],
            layout: "StandaloneLayout"
        });
    </script>
</body>
</html>"##;
