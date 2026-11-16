function(hongguo_picture_pool)
  get_target_property(sources vvdec SOURCES)
  list(FILTER sources EXCLUDE REGEX "/PicListManager\\.cpp$")
  list(APPEND sources "${HONGGUO_PICTURE_POOL_SOURCE}")
  set_property(TARGET vvdec PROPERTY SOURCES ${sources})
endfunction()

cmake_language(DEFER CALL hongguo_picture_pool)
