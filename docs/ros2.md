# ROS 2 Integration Boundary

SPACL v0.2.0 does not include a Robot Operating System 2 (ROS 2) adapter.

The adapter belongs below `RobotRuntime::execute`. It must accept a verified action object. It must map a fixed skill identifier to a fixed ROS 2 action or service type. It must not accept arbitrary topic names, shell commands, executable paths, or dynamic code.

A production adapter must implement these controls:

1. Allowlist each ROS 2 action, service, topic, and message type.
2. Convert token policy units to controller units without loss or overflow.
3. Check local safety state after token verification and before command release.
4. Bind feedback and completion to the token ID.
5. Stop work when token validity or safety state changes.
6. Sign the final controller result and relevant fault codes.
7. Apply Secure ROS 2 and Data Distribution Service (DDS) controls independently of SPACL.

The first adapter target is Gazebo. The second target is a hardware-in-the-loop mobile base or arm.
