# Tomato patch

Record monotonic transport arrival for buffered PDUs. Session bandwidth estimation no longer receives an absent timestamp (which reports zero bandwidth). Arrival metadata is bounded to 64 read spans; buffered PDUs share their transport arrival rather than decode completion time.
