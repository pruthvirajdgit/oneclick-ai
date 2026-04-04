// OneClick.ai Agent Tools Plugin
// Loaded by OpenClaw to provide schedule and notification capabilities.
//
// Environment variables expected:
//   ONECLICK_BACKEND_URL - Backend API base URL (default: http://backend:8080)
//   ONECLICK_AGENT_ID - This agent's UUID
//   ONECLICK_USER_ID - Owning user's UUID
//   ONECLICK_INTERNAL_SECRET - Shared secret for internal API auth

const BACKEND_URL = process.env.ONECLICK_BACKEND_URL || 'http://backend:8080';
const AGENT_ID = process.env.ONECLICK_AGENT_ID || '';
const USER_ID = process.env.ONECLICK_USER_ID || '';
const INTERNAL_SECRET = process.env.ONECLICK_INTERNAL_SECRET || '';

const headers = {
  'Content-Type': 'application/json',
  'X-Agent-Id': AGENT_ID,
  'X-User-Id': USER_ID,
  'X-Internal-Secret': INTERNAL_SECRET,
};

// Tool: create_schedule
async function createSchedule({ cron_expr, task_message }) {
  const res = await fetch(`${BACKEND_URL}/internal/schedules`, {
    method: 'POST',
    headers,
    body: JSON.stringify({ cron_expr, task_message }),
  });
  if (!res.ok) throw new Error(`Failed to create schedule: ${res.status}`);
  return await res.json();
}

// Tool: list_schedules
async function listSchedules() {
  const res = await fetch(`${BACKEND_URL}/internal/schedules`, {
    method: 'GET',
    headers,
  });
  if (!res.ok) throw new Error(`Failed to list schedules: ${res.status}`);
  return await res.json();
}

// Tool: delete_schedule
async function deleteSchedule({ schedule_id }) {
  const res = await fetch(`${BACKEND_URL}/internal/schedules/${schedule_id}`, {
    method: 'DELETE',
    headers,
  });
  if (!res.ok) throw new Error(`Failed to delete schedule: ${res.status}`);
  return { success: true };
}

// Tool: send_notification
async function sendNotification({ title, body }) {
  const res = await fetch(`${BACKEND_URL}/internal/notifications`, {
    method: 'POST',
    headers,
    body: JSON.stringify({ title, body }),
  });
  if (!res.ok) throw new Error(`Failed to send notification: ${res.status}`);
  return await res.json();
}

// Export tools for OpenClaw
module.exports = {
  tools: [
    {
      name: 'create_schedule',
      description: 'Create a recurring scheduled task',
      parameters: {
        type: 'object',
        properties: {
          cron_expr: { type: 'string', description: 'Cron expression (5-field)' },
          task_message: { type: 'string', description: 'Task message to execute' },
        },
        required: ['cron_expr', 'task_message'],
      },
      execute: createSchedule,
    },
    {
      name: 'list_schedules',
      description: 'List active scheduled tasks',
      parameters: { type: 'object', properties: {} },
      execute: listSchedules,
    },
    {
      name: 'delete_schedule',
      description: 'Delete a scheduled task by ID',
      parameters: {
        type: 'object',
        properties: {
          schedule_id: { type: 'string', description: 'UUID of schedule' },
        },
        required: ['schedule_id'],
      },
      execute: deleteSchedule,
    },
    {
      name: 'send_notification',
      description: 'Send notification to user dashboard',
      parameters: {
        type: 'object',
        properties: {
          title: { type: 'string', description: 'Notification title' },
          body: { type: 'string', description: 'Notification body' },
        },
        required: ['title', 'body'],
      },
      execute: sendNotification,
    },
  ],
};
