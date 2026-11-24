# Tomato changes

Completed fragmented DVC messages transfer their Vec allocation to the consumer
instead of allocating and copying through drain/collect. Total-size checks and
the handling of incomplete or oversized messages are unchanged.
